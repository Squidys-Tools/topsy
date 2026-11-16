//! PNG's chunk layer: dropping metadata without touching the image data, and reading or writing
//! the ICC profile and EXIF chunks.

use std::io::{self, Read, Write};

const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// Chunks that describe the file rather than the pixels.
const METADATA: [&[u8; 4]; 5] = [b"eXIf", b"tEXt", b"zTXt", b"iTXt", b"tIME"];

fn bad() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "This PNG is damaged")
}

pub struct Chunk<'a> {
    pub kind: [u8; 4],
    pub data: &'a [u8],
    pub raw: &'a [u8],
}

pub fn chunks(bytes: &[u8]) -> io::Result<Vec<Chunk<'_>>> {
    if !bytes.starts_with(SIGNATURE) {
        return Err(bad());
    }
    let mut out = Vec::new();
    let mut pos = 8;
    while pos < bytes.len() {
        let len = u32::from_be_bytes(
            bytes
                .get(pos..pos + 4)
                .ok_or_else(bad)?
                .try_into()
                .map_err(|_| bad())?,
        ) as usize;
        let end = pos
            .checked_add(12 + len)
            .filter(|&e| e <= bytes.len())
            .ok_or_else(bad)?;
        let kind: [u8; 4] = bytes[pos + 4..pos + 8].try_into().map_err(|_| bad())?;
        out.push(Chunk {
            kind,
            data: &bytes[pos + 8..pos + 8 + len],
            raw: &bytes[pos..end],
        });
        pos = end;
        if &kind == b"IEND" {
            break;
        }
    }
    Ok(out)
}

/// `bytes` without EXIF, text and timestamp chunks. Colour chunks (iCCP, sRGB, gAMA, cHRM) and
/// the image data stay byte for byte.
pub fn strip_metadata(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = SIGNATURE.to_vec();
    for c in chunks(bytes)? {
        if !METADATA.iter().any(|m| **m == c.kind) {
            out.extend_from_slice(c.raw);
        }
    }
    Ok(out)
}

pub fn exif(bytes: &[u8]) -> Option<Vec<u8>> {
    chunks(bytes)
        .ok()?
        .into_iter()
        .find(|c| &c.kind == b"eXIf")
        .map(|c| c.data.to_vec())
}

pub fn icc(bytes: &[u8]) -> Option<Vec<u8>> {
    let chunk = chunks(bytes)
        .ok()?
        .into_iter()
        .find(|c| &c.kind == b"iCCP")?;
    let name_end = chunk.data.iter().position(|&b| b == 0)?;
    let compressed = chunk.data.get(name_end + 2..)?;
    let mut icc = Vec::new();
    flate2::read::ZlibDecoder::new(compressed)
        .read_to_end(&mut icc)
        .ok()?;
    Some(icc)
}

pub fn crc(kind: &[u8], data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        std::array::from_fn(|n| {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            c
        })
    });
    let mut c = 0xFFFF_FFFFu32;
    for &b in kind.iter().chain(data) {
        c = table[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

pub fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc(kind, data).to_be_bytes());
}

/// An iCCP chunk payload for `icc`.
pub fn iccp(icc: &[u8]) -> Vec<u8> {
    let mut data = b"ICC Profile\0\0".to_vec();
    let mut z = flate2::write::ZlibEncoder::new(data, flate2::Compression::best());
    let _ = z.write_all(icc);
    data = z.finish().unwrap_or_default();
    data
}

/// `png` with iCCP and eXIf chunks for `icc`/`exif` placed after IHDR.
pub fn with_metadata(png: &[u8], icc: Option<&[u8]>, exif: Option<&[u8]>) -> io::Result<Vec<u8>> {
    let mut out = SIGNATURE.to_vec();
    for c in chunks(png)? {
        if (icc.is_some() && (&c.kind == b"iCCP" || &c.kind == b"sRGB"))
            || (exif.is_some() && &c.kind == b"eXIf")
        {
            continue;
        }
        out.extend_from_slice(c.raw);
        if &c.kind == b"IHDR" {
            if let Some(icc) = icc {
                write_chunk(&mut out, b"iCCP", &iccp(icc));
            }
            if let Some(exif) = exif {
                write_chunk(&mut out, b"eXIf", exif);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::exif::sample;

    fn png_with_everything() -> Vec<u8> {
        let mut buf = Vec::new();
        image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 4]))
            .write_to(&mut io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        let mut png = with_metadata(&buf, Some(b"a profile"), Some(&sample::exif(1))).unwrap();
        // Text and time chunks before IEND.
        let iend = png.len() - 12;
        let mut tail = Vec::new();
        write_chunk(&mut tail, b"tEXt", b"Author\0someone");
        write_chunk(&mut tail, b"iTXt", b"Comment\0\0\0\0\0hello");
        write_chunk(&mut tail, b"tIME", &[7, 0xE9, 1, 1, 0, 0, 0]);
        png.splice(iend..iend, tail);
        png
    }

    #[test]
    fn strips_exif_text_and_time_but_keeps_colour_and_pixels() {
        let png = png_with_everything();
        assert!(crate::model::exif::has_gps(&exif(&png).unwrap()));
        let stripped = strip_metadata(&png).unwrap();
        let kinds: Vec<[u8; 4]> = chunks(&stripped).unwrap().iter().map(|c| c.kind).collect();
        assert!(!kinds.iter().any(|k| METADATA.contains(&k)));
        assert_eq!(icc(&stripped).unwrap(), b"a profile");
        assert!(exif(&stripped).is_none());
        let a = image::load_from_memory(&png).unwrap();
        let b = image::load_from_memory(&stripped).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn crc_matches_a_known_chunk() {
        // IEND's CRC is fixed.
        assert_eq!(crc(b"IEND", &[]), 0xAE42_6082);
    }

    #[test]
    fn rejects_truncated_files() {
        let png = png_with_everything();
        assert!(chunks(&png[..png.len() - 20]).is_err());
        assert!(chunks(b"GIF89a").is_err());
    }
}
