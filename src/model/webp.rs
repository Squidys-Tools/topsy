//! WebP's RIFF container: dropping the EXIF and XMP chunks without touching the bitstream, and
//! reading the ICC profile and EXIF out.

use std::io;

const EXIF_FLAG: u8 = 0x08;
const XMP_FLAG: u8 = 0x04;

fn bad() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "This WebP is damaged")
}

struct Chunk<'a> {
    kind: [u8; 4],
    data: &'a [u8],
    raw: &'a [u8],
}

fn chunks(bytes: &[u8]) -> io::Result<Vec<Chunk<'_>>> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err(bad());
    }
    let mut out = Vec::new();
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let kind: [u8; 4] = bytes[pos..pos + 4].try_into().map_err(|_| bad())?;
        let len =
            u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().map_err(|_| bad())?) as usize;
        let data_end = pos
            .checked_add(8 + len)
            .filter(|&e| e <= bytes.len())
            .ok_or_else(bad)?;
        let end = (data_end + (len & 1)).min(bytes.len());
        out.push(Chunk {
            kind,
            data: &bytes[pos + 8..data_end],
            raw: &bytes[pos..end],
        });
        pos = end;
    }
    Ok(out)
}

/// `bytes` without its EXIF and XMP chunks, with the VP8X flags to match.
pub fn strip_metadata(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut body = Vec::with_capacity(bytes.len());
    for c in chunks(bytes)? {
        match &c.kind {
            b"EXIF" | b"XMP " => {}
            b"VP8X" => {
                let mut raw = c.raw.to_vec();
                if raw.len() > 8 {
                    raw[8] &= !(EXIF_FLAG | XMP_FLAG);
                }
                body.extend_from_slice(&raw);
            }
            _ => body.extend_from_slice(c.raw),
        }
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&body);
    Ok(out)
}

fn find(bytes: &[u8], kind: &[u8; 4]) -> Option<Vec<u8>> {
    chunks(bytes)
        .ok()?
        .into_iter()
        .find(|c| &c.kind == kind)
        .map(|c| c.data.to_vec())
}

pub fn exif(bytes: &[u8]) -> Option<Vec<u8>> {
    // Some writers keep JPEG's "Exif\0\0" prefix inside the chunk.
    find(bytes, b"EXIF").map(|e| e.strip_prefix(b"Exif\0\0").map(<[u8]>::to_vec).unwrap_or(e))
}

pub fn icc(bytes: &[u8]) -> Option<Vec<u8>> {
    find(bytes, b"ICCP")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::exif::sample;

    fn webp_with_metadata() -> Vec<u8> {
        let img = image::RgbaImage::from_fn(5, 3, |x, y| {
            image::Rgba([x as u8 * 40, y as u8 * 80, 7, 255])
        });
        let mut out = Vec::new();
        let mut enc = image_webp_lossy::WebPEncoder::new(&mut out);
        enc.set_icc_profile(b"a profile".to_vec());
        enc.set_exif_metadata(sample::exif(1));
        enc.set_xmp_metadata(b"<x:xmpmeta/>".to_vec());
        enc.encode(img.as_raw(), 5, 3, image_webp_lossy::ColorType::Rgba8)
            .unwrap();
        out
    }

    #[test]
    fn strips_exif_and_xmp_and_keeps_the_rest() {
        let webp = webp_with_metadata();
        assert!(crate::model::exif::has_gps(&exif(&webp).unwrap()));
        let stripped = strip_metadata(&webp).unwrap();
        assert!(exif(&stripped).is_none());
        assert!(find(&stripped, b"XMP ").is_none());
        assert_eq!(icc(&stripped).unwrap(), b"a profile");
        let vp8x = find(&stripped, b"VP8X").unwrap();
        assert_eq!(vp8x[0] & (EXIF_FLAG | XMP_FLAG), 0);
        let size = u32::from_le_bytes(stripped[4..8].try_into().unwrap()) as usize;
        assert_eq!(size + 8, stripped.len());
        let a = image::load_from_memory(&webp).unwrap();
        let b = image::load_from_memory(&stripped).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn rejects_other_files() {
        assert!(strip_metadata(b"RIFF\0\0\0\0WAVEfmt ").is_err());
    }
}
