//! JPEG's marker layer: the segments before and between scans, and where each scan's entropy
//! data ends. Everything that rewrites a JPEG without decoding it starts here.

use std::io;

pub const SOI: u8 = 0xD8;
pub const EOI: u8 = 0xD9;
pub const SOS: u8 = 0xDA;
pub const DQT: u8 = 0xDB;
pub const DHT: u8 = 0xC4;
pub const DRI: u8 = 0xDD;
pub const APP0: u8 = 0xE0;
pub const APP1: u8 = 0xE1;
pub const APP2: u8 = 0xE2;
#[cfg(any(test, feature = "test-support"))]
pub const APP13: u8 = 0xED;
pub const APP14: u8 = 0xEE;
pub const COM: u8 = 0xFE;

pub const EXIF_HEADER: &[u8] = b"Exif\0\0";
pub const ICC_HEADER: &[u8] = b"ICC_PROFILE\0";

pub fn bad(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("This JPEG is damaged ({what})"),
    )
}

/// One piece of a JPEG file. `data` is the payload after the length field; for a scan it is the
/// SOS header, and `scan` is the entropy-coded data that follows it.
#[derive(Debug, Clone, Copy)]
pub struct Segment<'a> {
    pub marker: u8,
    pub data: &'a [u8],
    pub scan: &'a [u8],
    /// The whole segment as it sits in the file, marker to the end of its scan data.
    pub raw: &'a [u8],
}

/// The segments of the first image in `bytes`, from the one after SOI up to and including EOI.
/// Anything after EOI (a phone's gain map or depth image) is not part of the list.
pub fn segments(bytes: &[u8]) -> io::Result<Vec<Segment<'_>>> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != SOI {
        return Err(bad("no start marker"));
    }
    let mut out = Vec::new();
    let mut pos = 2;
    loop {
        let start = pos;
        while pos < bytes.len() && bytes[pos] == 0xFF {
            pos += 1;
        }
        if pos >= bytes.len() || pos == start {
            return Err(bad("truncated"));
        }
        let marker = bytes[pos];
        pos += 1;
        if marker == EOI {
            out.push(Segment {
                marker,
                data: &[],
                scan: &[],
                raw: &bytes[start..pos],
            });
            return Ok(out);
        }
        if (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            out.push(Segment {
                marker,
                data: &[],
                scan: &[],
                raw: &bytes[start..pos],
            });
            continue;
        }
        if pos + 2 > bytes.len() {
            return Err(bad("truncated"));
        }
        let len = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]) as usize;
        if len < 2 || pos + len > bytes.len() {
            return Err(bad("segment overruns the file"));
        }
        let data = &bytes[pos + 2..pos + len];
        pos += len;
        let mut scan: &[u8] = &[];
        if marker == SOS {
            let end = entropy_end(bytes, pos);
            scan = &bytes[pos..end];
            pos = end;
        }
        out.push(Segment {
            marker,
            data,
            scan,
            raw: &bytes[start..pos],
        });
    }
}

/// Where the entropy-coded data starting at `pos` ends: the first marker that isn't a stuffed
/// 0xFF or a restart marker.
fn entropy_end(bytes: &[u8], mut pos: usize) -> usize {
    while pos + 1 < bytes.len() {
        if bytes[pos] == 0xFF {
            let next = bytes[pos + 1];
            if next != 0 && !(0xD0..=0xD7).contains(&next) && next != 0xFF {
                return pos;
            }
            pos += if next == 0xFF { 1 } else { 2 };
        } else {
            pos += 1;
        }
    }
    bytes.len()
}

/// Whether an APPn/COM segment carries metadata a "remove metadata" should drop. Kept: JFIF
/// (APP0), the ICC profile (APP2 `ICC_PROFILE`) and Adobe's colour transform flag (APP14), which
/// change how the pixels decode. Everything else (EXIF, XMP, IPTC, MPF, maker blocks, comments)
/// goes.
pub fn is_metadata(marker: u8, data: &[u8]) -> bool {
    match marker {
        APP0 => !(data.starts_with(b"JFIF\0") || data.starts_with(b"JFXX\0")),
        APP2 => !data.starts_with(ICC_HEADER),
        APP14 => !data.starts_with(b"Adobe"),
        0xE1..=0xEF | COM => true,
        _ => false,
    }
}

/// `bytes` without its metadata segments (see [`is_metadata`]) and without anything after EOI.
/// The image data is copied byte for byte.
pub fn strip_metadata(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let segments = segments(bytes)?;
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&[0xFF, SOI]);
    for s in &segments {
        if !is_metadata(s.marker, s.data) {
            out.extend_from_slice(s.raw);
        }
    }
    Ok(out)
}

/// The EXIF block (the TIFF structure after `Exif\0\0`), if any.
pub fn exif(segments: &[Segment]) -> Option<Vec<u8>> {
    segments
        .iter()
        .find(|s| s.marker == APP1 && s.data.starts_with(EXIF_HEADER))
        .map(|s| s.data[EXIF_HEADER.len()..].to_vec())
}

/// The ICC profile, reassembled from its numbered APP2 chunks.
pub fn icc(segments: &[Segment]) -> Option<Vec<u8>> {
    let mut chunks: Vec<(u8, &[u8])> = segments
        .iter()
        .filter(|s| s.marker == APP2 && s.data.len() > ICC_HEADER.len() + 2)
        .filter(|s| s.data.starts_with(ICC_HEADER))
        .map(|s| (s.data[ICC_HEADER.len()], &s.data[ICC_HEADER.len() + 2..]))
        .collect();
    if chunks.is_empty() {
        return None;
    }
    chunks.sort_by_key(|c| c.0);
    Some(
        chunks
            .into_iter()
            .flat_map(|c| c.1.iter().copied())
            .collect(),
    )
}

/// Appends one segment (marker, length, payload) to `out`.
pub fn write_segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    out.extend_from_slice(&[0xFF, marker]);
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
}

/// The APP1 EXIF segment payload for `exif`, or `None` if it can't fit in one segment.
pub fn exif_segment(exif: &[u8]) -> Option<Vec<u8>> {
    let payload: Vec<u8> = EXIF_HEADER.iter().chain(exif).copied().collect();
    (payload.len() <= 65533).then_some(payload)
}

/// `bytes` with an EXIF block added after SOI and any JFIF header.
pub fn with_exif(bytes: &[u8], exif: &[u8]) -> io::Result<Vec<u8>> {
    let payload = exif_segment(exif).ok_or_else(|| bad("EXIF too large"))?;
    let mut at = 2;
    if bytes.get(2..4) == Some(&[0xFF, APP0]) {
        let len = bytes
            .get(4..6)
            .map(|l| usize::from(u16::from_be_bytes([l[0], l[1]])));
        at += 2 + len.ok_or_else(|| bad("JFIF header"))?;
    }
    let head = bytes.get(..at).ok_or_else(|| bad("JFIF header"))?;
    let mut out = head.to_vec();
    write_segment(&mut out, APP1, &payload);
    out.extend_from_slice(&bytes[at..]);
    Ok(out)
}

/// The ICC profile split into APP2 payloads.
#[cfg(any(test, feature = "test-support"))]
pub fn icc_segments(icc: &[u8]) -> Vec<Vec<u8>> {
    const MAX: usize = 65533 - 14;
    let chunks: Vec<&[u8]> = icc.chunks(MAX).collect();
    if chunks.len() > 255 {
        return Vec::new();
    }
    let count = chunks.len() as u8;
    chunks
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let mut p = ICC_HEADER.to_vec();
            p.push(i as u8 + 1);
            p.push(count);
            p.extend_from_slice(c);
            p
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample() -> Vec<u8> {
        let mut f = vec![0xFF, SOI];
        write_segment(&mut f, APP0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        write_segment(
            &mut f,
            APP1,
            &exif_segment(b"MM\0\x2a\0\0\0\x08\0\0").unwrap(),
        );
        write_segment(&mut f, APP1, b"http://ns.adobe.com/xap/1.0/\0<x/>");
        for p in icc_segments(&[7u8; 100]) {
            write_segment(&mut f, APP2, &p);
        }
        write_segment(&mut f, APP2, b"MPF\0data");
        write_segment(&mut f, APP13, b"Photoshop 3.0\0IPTC");
        write_segment(&mut f, COM, b"a comment");
        write_segment(&mut f, APP14, b"Adobe\0\x64\0\0\0\0\x01");
        write_segment(&mut f, DQT, &[0; 65]);
        write_segment(&mut f, SOS, &[1, 1, 0, 0, 63, 0]);
        f.extend_from_slice(&[0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56]);
        f.extend_from_slice(&[0xFF, EOI]);
        f.extend_from_slice(b"\xFF\xD8trailing gain map");
        f
    }

    #[test]
    fn scans_run_past_stuffing_and_restarts() {
        let f = sample();
        let segs = segments(&f).unwrap();
        let sos = segs.iter().find(|s| s.marker == SOS).unwrap();
        assert_eq!(sos.scan, &[0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56]);
        assert_eq!(segs.last().unwrap().marker, EOI);
    }

    #[test]
    fn strip_keeps_jfif_icc_adobe_and_the_scan() {
        let f = sample();
        let stripped = strip_metadata(&f).unwrap();
        let segs = segments(&stripped).unwrap();
        let kinds: Vec<u8> = segs.iter().map(|s| s.marker).collect();
        assert_eq!(kinds, vec![APP0, APP2, APP14, DQT, SOS, EOI]);
        assert_eq!(icc(&segs).unwrap(), vec![7u8; 100]);
        assert!(exif(&segs).is_none());
        assert!(stripped.ends_with(&[0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56, 0xFF, EOI]));
        assert!(!stripped.windows(7).any(|w| w == b"comment"));
        // Stripping twice changes nothing.
        assert_eq!(strip_metadata(&stripped).unwrap(), stripped);
    }

    #[test]
    fn large_icc_profiles_split_and_reassemble() {
        let profile: Vec<u8> = (0..150_000u32).map(|i| (i % 251) as u8).collect();
        let mut f = vec![0xFF, SOI];
        for p in icc_segments(&profile) {
            write_segment(&mut f, APP2, &p);
        }
        f.extend_from_slice(&[0xFF, EOI]);
        assert_eq!(icc(&segments(&f).unwrap()).unwrap(), profile);
    }

    #[test]
    fn rejects_garbage() {
        assert!(segments(b"not a jpeg").is_err());
        assert!(segments(&[0xFF, SOI, 0xFF, APP1, 0x40, 0]).is_err());
    }
}
