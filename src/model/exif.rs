//! Just enough of EXIF's TIFF structure to read a photo's orientation and to bring the tags a
//! transform makes stale (orientation, pixel dimensions) back in line, patched in place so the
//! rest of the block (camera, date, GPS) survives byte for byte.

const ORIENTATION: u16 = 0x0112;
const EXIF_IFD: u16 = 0x8769;
const GPS_IFD: u16 = 0x8825;
const PIXEL_X: u16 = 0xA002;
const PIXEL_Y: u16 = 0xA003;

struct Tiff {
    big: bool,
}

impl Tiff {
    fn new(b: &[u8]) -> Option<Self> {
        match b.get(..4)? {
            [b'I', b'I', 42, 0] => Some(Self { big: false }),
            [b'M', b'M', 0, 42] => Some(Self { big: true }),
            _ => None,
        }
    }

    fn u16(&self, b: &[u8], at: usize) -> Option<u16> {
        let s = b.get(at..at + 2)?;
        Some(if self.big {
            u16::from_be_bytes([s[0], s[1]])
        } else {
            u16::from_le_bytes([s[0], s[1]])
        })
    }

    fn u32(&self, b: &[u8], at: usize) -> Option<u32> {
        let s: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
        Some(if self.big {
            u32::from_be_bytes(s)
        } else {
            u32::from_le_bytes(s)
        })
    }

    /// The offsets of each entry (12 bytes: tag, type, count, value) of the IFD at `ifd`.
    fn entries(&self, b: &[u8], ifd: usize) -> Vec<usize> {
        let Some(n) = self.u16(b, ifd) else {
            return Vec::new();
        };
        (0..n as usize)
            .map(|i| ifd + 2 + 12 * i)
            .take_while(|&e| e + 12 <= b.len())
            .collect()
    }

    fn find(&self, b: &[u8], ifd: usize, tag: u16) -> Option<usize> {
        self.entries(b, ifd)
            .into_iter()
            .find(|&e| self.u16(b, e) == Some(tag))
    }

    fn ifd0(&self, b: &[u8]) -> Option<usize> {
        self.u32(b, 4).map(|o| o as usize)
    }

    fn sub_ifd(&self, b: &[u8], tag: u16) -> Option<usize> {
        let e = self.find(b, self.ifd0(b)?, tag)?;
        self.u32(b, e + 8).map(|o| o as usize)
    }

    /// Writes an inline SHORT or LONG value.
    fn set(&self, b: &mut [u8], entry: usize, value: u32) {
        let Some(kind) = self.u16(b, entry + 2) else {
            return;
        };
        let at = entry + 8;
        match kind {
            3 if value <= 0xFFFF => {
                let v = value as u16;
                let bytes = if self.big {
                    v.to_be_bytes()
                } else {
                    v.to_le_bytes()
                };
                b[at..at + 2].copy_from_slice(&bytes);
            }
            4 => {
                let bytes = if self.big {
                    value.to_be_bytes()
                } else {
                    value.to_le_bytes()
                };
                b[at..at + 4].copy_from_slice(&bytes);
            }
            _ => {}
        }
    }
}

/// The orientation tag (1..=8), or `None` if the block has none or isn't TIFF.
pub fn orientation(exif: &[u8]) -> Option<u16> {
    let t = Tiff::new(exif)?;
    let e = t.find(exif, t.ifd0(exif)?, ORIENTATION)?;
    t.u16(exif, e + 8).filter(|v| (1..=8).contains(v))
}

/// Sets the orientation to upright and the recorded pixel size to `size`, where those tags exist.
pub fn normalise(exif: &mut [u8], size: Option<(u32, u32)>) {
    let Some(t) = Tiff::new(exif) else { return };
    if let Some(ifd0) = t.ifd0(exif)
        && let Some(e) = t.find(exif, ifd0, ORIENTATION)
    {
        t.set(exif, e, 1);
    }
    if let (Some((w, h)), Some(sub)) = (size, t.sub_ifd(exif, EXIF_IFD)) {
        for (tag, v) in [(PIXEL_X, w), (PIXEL_Y, h)] {
            if let Some(e) = t.find(exif, sub, tag) {
                t.set(exif, e, v);
            }
        }
    }
}

/// Whether the block records a location.
/// An EXIF block holding nothing but `orientation`, for a photo stripped of its metadata that
/// still needs telling which way is up.
pub fn orientation_only(orientation: u16) -> Vec<u8> {
    let mut b = vec![b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 1];
    b.extend_from_slice(&ORIENTATION.to_be_bytes());
    b.extend_from_slice(&[0, 3, 0, 0, 0, 1]);
    b.extend_from_slice(&orientation.to_be_bytes());
    b.extend_from_slice(&[0; 6]);
    b
}

pub fn has_gps(exif: &[u8]) -> bool {
    Tiff::new(exif).is_some_and(|t| {
        t.sub_ifd(exif, GPS_IFD)
            .is_some_and(|g| !t.entries(exif, g).is_empty())
    })
}

#[cfg(test)]
pub mod sample {
    /// A big-endian EXIF block with orientation `o`, pixel size 640x480 and a GPS IFD.
    pub fn exif(o: u16) -> Vec<u8> {
        let mut b = b"MM\0\x2a\0\0\0\x08".to_vec();
        // IFD0 at 8: 3 entries, then next-IFD 0.
        b.extend_from_slice(&3u16.to_be_bytes());
        let entry = |b: &mut Vec<u8>, tag: u16, kind: u16, value: u32| {
            b.extend_from_slice(&tag.to_be_bytes());
            b.extend_from_slice(&kind.to_be_bytes());
            b.extend_from_slice(&1u32.to_be_bytes());
            if kind == 3 {
                b.extend_from_slice(&(value as u16).to_be_bytes());
                b.extend_from_slice(&[0, 0]);
            } else {
                b.extend_from_slice(&value.to_be_bytes());
            }
        };
        let ifd0_end = 8 + 2 + 3 * 12 + 4;
        let exif_ifd = ifd0_end;
        let gps_ifd = exif_ifd + 2 + 2 * 12 + 4;
        entry(&mut b, 0x0112, 3, u32::from(o));
        entry(&mut b, 0x8769, 4, exif_ifd as u32);
        entry(&mut b, 0x8825, 4, gps_ifd as u32);
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&2u16.to_be_bytes());
        entry(&mut b, 0xA002, 4, 640);
        entry(&mut b, 0xA003, 3, 480);
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&1u16.to_be_bytes());
        entry(&mut b, 0x0002, 4, 0x1234_5678); // GPSLatitude, the value doesn't matter here
        b.extend_from_slice(&[0; 4]);
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_orientation_only_block_reads_back() {
        let e = orientation_only(6);
        assert_eq!(orientation(&e), Some(6));
        assert!(!has_gps(&e));
    }

    #[test]
    fn reads_and_resets_orientation_and_size() {
        let mut e = sample::exif(6);
        assert_eq!(orientation(&e), Some(6));
        assert!(has_gps(&e));
        normalise(&mut e, Some((480, 640)));
        assert_eq!(orientation(&e), Some(1));
        let t = Tiff::new(&e).unwrap();
        let sub = t.sub_ifd(&e, EXIF_IFD).unwrap();
        let x = t.find(&e, sub, PIXEL_X).unwrap();
        let y = t.find(&e, sub, PIXEL_Y).unwrap();
        assert_eq!(t.u32(&e, x + 8), Some(480));
        assert_eq!(t.u16(&e, y + 8), Some(640));
        assert!(
            has_gps(&e),
            "the location is the user's to remove, not a rotation's"
        );
    }

    #[test]
    fn little_endian_and_garbage() {
        let le = b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x08\0\0\0\0\0\0\0".to_vec();
        assert_eq!(orientation(&le), Some(8));
        assert_eq!(orientation(b"nonsense"), None);
        assert_eq!(orientation(b"MM\0\x2a\xff\xff\xff\xff"), None);
        assert!(!has_gps(&le));
        let mut junk = b"MM\0\x2a\0\0\0\x08\xff\xff".to_vec();
        normalise(&mut junk, Some((1, 1)));
    }
}
