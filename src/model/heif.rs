//! Enough of HEIF/AVIF's box structure (ISO/IEC 23008-12) to lift out the EXIF item and the ICC
//! profile, which WIC decodes past but doesn't hand over, so a converted iPhone photo keeps its
//! date, camera and location.

struct Boxes<'a> {
    data: &'a [u8],
    pos: usize,
}

/// The (type, payload) boxes in `data`.
fn boxes(data: &[u8]) -> Boxes<'_> {
    Boxes { data, pos: 0 }
}

impl<'a> Iterator for Boxes<'a> {
    type Item = ([u8; 4], &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let d = self.data;
        let p = self.pos;
        let size = u32::from_be_bytes(d.get(p..p + 4)?.try_into().ok()?) as usize;
        let kind: [u8; 4] = d.get(p + 4..p + 8)?.try_into().ok()?;
        let (header, size) = match size {
            0 => (8, d.len() - p),
            1 => (
                16,
                u64::from_be_bytes(d.get(p + 8..p + 16)?.try_into().ok()?) as usize,
            ),
            n => (8, n),
        };
        let end = p.checked_add(size)?;
        if size < header || end > d.len() {
            return None;
        }
        self.pos = end;
        Some((kind, &d[p + header..end]))
    }
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data).find(|(k, _)| k == kind).map(|(_, b)| b)
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn uint(&mut self, bytes: usize) -> Option<u64> {
        let s = self.d.get(self.p..self.p + bytes)?;
        self.p += bytes;
        Some(s.iter().fold(0u64, |a, &b| (a << 8) | u64::from(b)))
    }
}

fn meta(file: &[u8]) -> Option<&[u8]> {
    // `meta` is a full box: four bytes of version and flags first.
    child(file, b"meta")?.get(4..)
}

/// The id of the first item of type `kind`.
fn item_id(meta: &[u8], kind: &[u8; 4]) -> Option<u32> {
    let iinf = child(meta, b"iinf")?;
    let skip = if iinf.first()? == &0 { 6 } else { 8 };
    for (k, infe) in boxes(iinf.get(skip..)?) {
        if &k != b"infe" {
            continue;
        }
        let version = *infe.first()?;
        let mut r = Reader { d: infe, p: 4 };
        let id = r.uint(if version >= 3 { 4 } else { 2 })? as u32;
        r.uint(2)?;
        if version >= 2 && infe.get(r.p..r.p + 4)? == kind {
            return Some(id);
        }
    }
    None
}

/// The bytes of item `id`, joined from its extents.
fn item_data(file: &[u8], meta: &[u8], id: u32) -> Option<Vec<u8>> {
    let iloc = child(meta, b"iloc")?;
    let version = *iloc.first()?;
    let mut r = Reader { d: iloc, p: 4 };
    let sizes = r.uint(2)?;
    let (off, len, base, index) = (
        (sizes >> 12) as usize & 15,
        (sizes >> 8) as usize & 15,
        (sizes >> 4) as usize & 15,
        sizes as usize & 15,
    );
    let count = r.uint(if version < 2 { 2 } else { 4 })?;
    for _ in 0..count {
        let item = r.uint(if version < 2 { 2 } else { 4 })? as u32;
        let method = if version >= 1 { r.uint(2)? & 15 } else { 0 };
        r.uint(2)?;
        let base_offset = r.uint(base)? as usize;
        let extents = r.uint(2)?;
        let mut out = Vec::new();
        for _ in 0..extents {
            if version >= 1 && index > 0 {
                r.uint(index)?;
            }
            let o = base_offset + r.uint(off)? as usize;
            let l = r.uint(len)? as usize;
            let source: &[u8] = match method {
                0 => file,
                1 => child(meta, b"idat")?,
                _ => return None,
            };
            let l = if l == 0 {
                source.len().checked_sub(o)?
            } else {
                l
            };
            out.extend_from_slice(source.get(o..o.checked_add(l)?)?);
        }
        if item == id {
            return Some(out);
        }
    }
    None
}

/// The EXIF block (TIFF structure), if the file has one.
pub fn exif(file: &[u8]) -> Option<Vec<u8>> {
    let meta = meta(file)?;
    let data = item_data(file, meta, item_id(meta, b"Exif")?)?;
    let offset = u32::from_be_bytes(data.get(..4)?.try_into().ok()?) as usize;
    data.get(4 + offset..).map(<[u8]>::to_vec)
}

/// The ICC profile from the first `colr` property that carries one.
pub fn icc(file: &[u8]) -> Option<Vec<u8>> {
    let ipco = child(child(meta(file)?, b"iprp")?, b"ipco")?;
    boxes(ipco)
        .filter(|(k, _)| k == b"colr")
        .find(|(_, b)| matches!(b.get(..4), Some(b"prof") | Some(b"rICC")))
        .map(|(_, b)| b[4..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::exif::sample;

    fn full_box(kind: &[u8; 4], version: u8, body: &[u8]) -> Vec<u8> {
        let mut b = vec![version, 0, 0, 0];
        b.extend_from_slice(body);
        plain_box(kind, &b)
    }

    fn plain_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut b = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        b.extend_from_slice(kind);
        b.extend_from_slice(body);
        b
    }

    /// A HEIF skeleton with an EXIF item in `mdat` (or `idat`) and an ICC `colr` box.
    fn heif(in_idat: bool) -> Vec<u8> {
        let mut exif_item = 6u32.to_be_bytes().to_vec();
        exif_item.extend_from_slice(b"Exif\0\0");
        exif_item.extend_from_slice(&sample::exif(1));
        let mut infe_hvc = 1u16.to_be_bytes().to_vec();
        infe_hvc.extend_from_slice(&[0, 0]);
        infe_hvc.extend_from_slice(b"hvc1\0");
        let mut infe_exif = 2u16.to_be_bytes().to_vec();
        infe_exif.extend_from_slice(&[0, 0]);
        infe_exif.extend_from_slice(b"Exif\0");
        let mut iinf_body = 2u16.to_be_bytes().to_vec();
        iinf_body.extend(full_box(b"infe", 2, &infe_hvc));
        iinf_body.extend(full_box(b"infe", 2, &infe_exif));
        let iinf = full_box(b"iinf", 0, &iinf_body);
        let colr = plain_box(b"colr", &[b"prof".as_slice(), b"icc bytes"].concat());
        let iprp = plain_box(b"iprp", &plain_box(b"ipco", &colr));
        let ftyp = plain_box(b"ftyp", b"heic\0\0\0\0mif1heic");
        // iloc v1: offset 4, length 4, base 0, index 0; one item.
        let build = |offset: u32| {
            let mut body = vec![0x44, 0x00];
            body.extend_from_slice(&1u16.to_be_bytes());
            body.extend_from_slice(&2u16.to_be_bytes());
            body.extend_from_slice(&(u16::from(in_idat)).to_be_bytes());
            body.extend_from_slice(&0u16.to_be_bytes());
            body.extend_from_slice(&1u16.to_be_bytes());
            body.extend_from_slice(&offset.to_be_bytes());
            body.extend_from_slice(&(exif_item.len() as u32).to_be_bytes());
            let mut meta_body = Vec::new();
            meta_body.extend(full_box(
                b"hdlr",
                0,
                b"\0\0\0\0pict\0\0\0\0\0\0\0\0\0\0\0\0\0",
            ));
            meta_body.extend(iinf.clone());
            meta_body.extend(full_box(b"iloc", 1, &body));
            meta_body.extend(iprp.clone());
            if in_idat {
                meta_body.extend(plain_box(b"idat", &exif_item));
            }
            full_box(b"meta", 0, &meta_body)
        };
        let meta_len = build(0).len();
        let offset = if in_idat {
            0
        } else {
            (ftyp.len() + meta_len + 8) as u32
        };
        let mut file = ftyp.clone();
        file.extend(build(offset));
        file.extend(plain_box(b"mdat", if in_idat { b"" } else { &exif_item }));
        file
    }

    #[test]
    fn finds_exif_in_mdat_and_idat_and_the_icc_profile() {
        for in_idat in [false, true] {
            let f = heif(in_idat);
            let e = exif(&f).expect("exif");
            assert!(crate::model::exif::has_gps(&e), "idat: {in_idat}");
            assert_eq!(icc(&f).unwrap(), b"icc bytes");
        }
    }

    #[test]
    fn garbage_gives_nothing() {
        assert!(exif(b"\0\0\0\x10ftypheic\0\0\0\0").is_none());
        assert!(icc(&[0xFF; 64]).is_none());
        let mut f = heif(false);
        f.truncate(f.len() - 30);
        assert!(exif(&f).is_none());
    }
}
