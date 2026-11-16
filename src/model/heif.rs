//! HEIC written by topsy: FFmpeg's x265 encodes the picture (and its alpha plane) as one-frame
//! MP4s, and the coded frames move into a HEIF container built here. The portable ImageMagick
//! reads HEIC but has no HEVC encoder.

/// One coded HEVC frame: its decoder configuration (the `hvcC` box's body) and its NAL units,
/// each behind a 4-byte length, as MP4 stores them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Still {
    pub config: Vec<u8>,
    pub data: Vec<u8>,
}

/// Which plane an encode writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    Colour,
    Alpha,
}

/// x265's rate factor for a 1..=100 quality: 75 is 22, which looks like a JPEG at 75 a quarter
/// its size.
pub fn crf(quality: u8) -> u8 {
    (51.0 - f32::from(quality.clamp(1, 100)) * 0.39).round() as u8
}

/// The size x265 codes a `width` x `height` picture at: 4:2:0 needs even sides, and `clap`
/// crops the padding off again.
pub fn coded(width: u32, height: u32) -> (u32, u32) {
    (width + width % 2, height + height % 2)
}

/// FFmpeg's arguments to encode one plane of the PNG at `png` into a one-frame MP4 at `out`.
pub fn encode_args(png: &str, out: &str, crf: u8, plane: Plane) -> Vec<String> {
    let pad = "pad=ceil(iw/2)*2:ceil(ih/2)*2";
    let (filter, pixels) = match plane {
        Plane::Colour => (
            format!(
                "{pad},scale=out_color_matrix=bt709:out_range=pc:flags=lanczos+accurate_rnd+full_chroma_int,format=yuv420p"
            ),
            &[
                "-color_range",
                "pc",
                "-colorspace",
                "bt709",
                "-color_primaries",
                "bt709",
                "-color_trc",
                "iec61966-2-1",
            ][..],
        ),
        Plane::Alpha => (
            format!("alphaextract,{pad},format=gray"),
            &["-color_range", "pc"][..],
        ),
    };
    let mut a: Vec<String> = [
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "error",
        "-y",
        "-i",
        png,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let crf = crf.to_string();
    let rest = [
        "-frames:v",
        "1",
        "-vf",
        &filter,
        "-c:v",
        "libx265",
        "-preset",
        "medium",
        "-crf",
        &crf,
        "-x265-params",
        "log-level=error",
    ];
    a.extend(rest.iter().chain(pixels).map(|s| s.to_string()));
    a.extend(
        ["-tag:v", "hvc1", "-f", "mp4", out]
            .iter()
            .map(|s| s.to_string()),
    );
    a
}

/// The first frame of the first track of an MP4 holding HEVC.
pub fn from_mp4(mp4: &[u8]) -> Option<Still> {
    let moov = child(mp4, b"moov")?;
    let stbl = ["trak", "mdia", "minf", "stbl"]
        .iter()
        .try_fold(moov, |b, k| child(b, k.as_bytes().try_into().ok()?))?;
    // stsd: version and flags, a count, then the sample entry: its header, 8 bytes of
    // SampleEntry and 70 of VisualSampleEntry before its own boxes.
    let entry = child(stbl, b"stsd")?.get(8..)?;
    let entry = entry.get(8..be32(entry, 0)? as usize)?;
    let config = child(entry.get(78..)?, b"hvcC")?.to_vec();
    let stsz = child(stbl, b"stsz")?;
    let size = match be32(stsz, 4)? {
        0 => be32(stsz, 12)?,
        size => size,
    } as usize;
    let offset = match child(stbl, b"stco") {
        Some(stco) => u64::from(be32(stco, 8)?),
        None => {
            let co64 = child(stbl, b"co64")?;
            u64::from_be_bytes(co64.get(8..16)?.try_into().ok()?)
        }
    } as usize;
    let data = mp4.get(offset..offset.checked_add(size)?)?.to_vec();
    Some(Still { config, data })
}

/// What goes into a HEIC.
pub struct Heic<'a> {
    /// The picture's own size; the coded one is `coded(width, height)`.
    pub width: u32,
    pub height: u32,
    pub colour: &'a Still,
    pub alpha: Option<&'a Still>,
    /// The ICC profile the pixels are in; without one they are sRGB.
    pub icc: Option<&'a [u8]>,
    /// The EXIF block, with or without its `Exif\0\0` mark.
    pub exif: Option<&'a [u8]>,
}

const COLOUR: u16 = 1;
const ALPHA: u16 = 2;
const EXIF: u16 = 3;

pub fn write(h: &Heic) -> Vec<u8> {
    let ftyp = boxed(b"ftyp", b"heic\0\0\0\0mif1heic");
    let exif = h.exif.map(|e| {
        let mark = if e.starts_with(b"Exif\0\0") { 6 } else { 0 };
        [&(mark as u32).to_be_bytes()[..], e].concat()
    });
    let mut items: Vec<(u16, &[u8])> = vec![(COLOUR, &h.colour.data)];
    if let Some(a) = h.alpha {
        items.push((ALPHA, &a.data));
    }
    if let Some(e) = &exif {
        items.push((EXIF, e));
    }
    // The offsets in iloc are absolute, and meta's size doesn't depend on their values.
    let draft = meta(h, &items, 0);
    let start = (ftyp.len() + draft.len() + 8) as u32;
    let meta = meta(h, &items, start);
    let body: Vec<u8> = items.iter().flat_map(|(_, d)| d.iter().copied()).collect();
    [ftyp, meta, boxed(b"mdat", &body)].concat()
}

fn meta(h: &Heic, items: &[(u16, &[u8])], start: u32) -> Vec<u8> {
    let hdlr = full(b"hdlr", 0, &[&[0; 4][..], b"pict", &[0; 13]].concat());
    let pitm = full(b"pitm", 0, &COLOUR.to_be_bytes());
    let infe = |id: u16, kind: &[u8; 4]| {
        full(
            b"infe",
            2,
            &[&id.to_be_bytes()[..], &[0, 0], kind, &[0]].concat(),
        )
    };
    let mut iinf = (items.len() as u16).to_be_bytes().to_vec();
    for &(id, _) in items {
        iinf.extend(infe(id, if id == EXIF { b"Exif" } else { b"hvc1" }));
    }
    let mut iref = Vec::new();
    for (id, kind) in [(ALPHA, b"auxl"), (EXIF, b"cdsc")] {
        if items.iter().any(|&(i, _)| i == id) {
            let refs = [id.to_be_bytes(), 1u16.to_be_bytes(), COLOUR.to_be_bytes()].concat();
            iref.extend(boxed(kind, &refs));
        }
    }
    let mut iloc = vec![0x44, 0x00];
    iloc.extend((items.len() as u16).to_be_bytes());
    let mut at = start;
    for &(id, data) in items {
        iloc.extend([&id.to_be_bytes()[..], &[0, 0], &1u16.to_be_bytes()].concat());
        iloc.extend([at.to_be_bytes(), (data.len() as u32).to_be_bytes()].concat());
        at += data.len() as u32;
    }
    let mut parts = vec![hdlr, pitm, full(b"iinf", 0, &iinf)];
    if !iref.is_empty() {
        parts.push(full(b"iref", 0, &iref));
    }
    parts.push(boxed(b"iprp", &properties(h)));
    parts.push(full(b"iloc", 0, &iloc));
    full(b"meta", 0, &parts.concat())
}

/// `ipco`, then `ipma` tying the properties to the colour and alpha items. Indices are 1-based;
/// the high bit marks a property a reader must understand.
fn properties(h: &Heic) -> Vec<u8> {
    let (cw, ch) = coded(h.width, h.height);
    let ispe = full(b"ispe", 0, &[cw.to_be_bytes(), ch.to_be_bytes()].concat());
    let (primaries, transfer) = if h.icc.is_some() {
        (2u16, 2u16)
    } else {
        (1, 13)
    };
    let nclx = [
        &b"nclx"[..],
        &primaries.to_be_bytes(),
        &transfer.to_be_bytes(),
        &[0, 1, 0x80],
    ];
    let mut props = vec![
        boxed(b"hvcC", &h.colour.config),
        ispe,
        boxed(b"colr", &nclx.concat()),
        full(b"pixi", 0, &[3, 8, 8, 8]),
    ];
    let mut colour = vec![0x81, 2, 3, 4];
    if let Some(icc) = h.icc {
        props.push(boxed(b"colr", &[&b"prof"[..], icc].concat()));
        colour.push(props.len() as u8);
    }
    let mut clap = None;
    if (cw, ch) != (h.width, h.height) {
        // Width and height, then the centre's offset, each a fraction: (clean - coded) / 2.
        let half = |clean: u32, coded: u32| [(clean as i32 - coded as i32), 2];
        let parts = [
            [h.width as i32, 1],
            [h.height as i32, 1],
            half(h.width, cw),
            half(h.height, ch),
        ];
        let bytes: Vec<u8> = parts
            .iter()
            .flatten()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        props.push(boxed(b"clap", &bytes));
        clap = Some(0x80 | props.len() as u8);
        colour.extend(clap);
    }
    let mut ipma = Vec::new();
    let mut entry = |id: u16, list: &[u8]| {
        ipma.extend(id.to_be_bytes());
        ipma.push(list.len() as u8);
        ipma.extend(list);
    };
    let alpha = h.alpha.map(|a| {
        props.push(boxed(b"hvcC", &a.config));
        let mut list = vec![0x80 | props.len() as u8, 2];
        props.push(full(b"auxC", 0, b"urn:mpeg:hevc:2015:auxid:1\0"));
        list.push(0x80 | props.len() as u8);
        props.push(full(b"pixi", 0, &[1, 8]));
        list.push(props.len() as u8);
        list.extend(clap);
        list
    });
    entry(COLOUR, &colour);
    if let Some(list) = &alpha {
        entry(ALPHA, list);
    }
    let count = 1 + u32::from(alpha.is_some());
    let ipma = full(b"ipma", 0, &[&count.to_be_bytes()[..], &ipma].concat());
    [boxed(b"ipco", &props.concat()), ipma].concat()
}

fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [&((body.len() + 8) as u32).to_be_bytes()[..], kind, body].concat()
}

fn full(kind: &[u8; 4], version: u8, body: &[u8]) -> Vec<u8> {
    boxed(kind, &[&[version, 0, 0, 0][..], body].concat())
}

/// The body of the first box of `kind` among the boxes in `data`.
fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    let mut at = 0;
    while at + 8 <= data.len() {
        let size = be32(data, at)? as usize;
        let (head, size) = match size {
            1 => (
                16,
                u64::from_be_bytes(data.get(at + 8..at + 16)?.try_into().ok()?) as usize,
            ),
            0 => (8, data.len() - at),
            size => (8, size),
        };
        let end = at
            .checked_add(size)
            .filter(|&e| e <= data.len() && size >= head)?;
        if &data[at + 4..at + 8] == kind {
            return data.get(at + head..end);
        }
        at = end;
    }
    None
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn still(tag: u8) -> Still {
        Still {
            config: vec![1, tag, 3],
            data: vec![0, 0, 0, 2, tag, tag],
        }
    }

    fn path<'a>(data: &'a [u8], kinds: &[(&[u8; 4], usize)]) -> &'a [u8] {
        kinds
            .iter()
            .fold(data, |b, (k, skip)| &child(b, k).unwrap()[*skip..])
    }

    #[test]
    fn quality_maps_to_a_rate_factor() {
        assert_eq!((crf(75), crf(90), crf(1), crf(0)), (22, 16, 51, 51));
        assert_eq!(coded(1001, 625), (1002, 626));
        let colour = encode_args("a.png", "o.mp4", 22, Plane::Colour).join(" ");
        assert!(
            colour.contains("-vf pad=ceil(iw/2)*2:ceil(ih/2)*2,scale=out_color_matrix=bt709"),
            "{colour}"
        );
        assert!(
            colour.ends_with(
                "-crf 22 -x265-params log-level=error -color_range pc -colorspace bt709 \
             -color_primaries bt709 -color_trc iec61966-2-1 -tag:v hvc1 -f mp4 o.mp4"
            ),
            "{colour}"
        );
        let alpha = encode_args("a.png", "o.mp4", 22, Plane::Alpha).join(" ");
        assert!(alpha.contains("-vf alphaextract,pad="), "{alpha}");
    }

    #[test]
    fn the_frame_comes_out_of_an_mp4() {
        let sample = b"\0\0\0\x03abc".to_vec();
        let mut entry = vec![0; 78];
        entry.extend(boxed(b"hvcC", &[1, 2, 3]));
        let stsd = full(
            b"stsd",
            0,
            &[&1u32.to_be_bytes()[..], &boxed(b"hvc1", &entry)].concat(),
        );
        let stsz = full(
            b"stsz",
            0,
            &[0u32.to_be_bytes(), 1u32.to_be_bytes(), 7u32.to_be_bytes()].concat(),
        );
        let mdat = boxed(b"mdat", &sample);
        let stco = full(
            b"stco",
            0,
            &[1u32.to_be_bytes(), 8u32.to_be_bytes()].concat(),
        );
        let stbl = boxed(b"stbl", &[stsd, stsz, stco].concat());
        let moov = ["minf", "mdia", "trak", "moov"]
            .iter()
            .fold(stbl, |inner, k| {
                boxed(k.as_bytes().try_into().unwrap(), &inner)
            });
        let mp4 = [mdat, moov].concat();
        assert_eq!(
            from_mp4(&mp4),
            Some(Still {
                config: vec![1, 2, 3],
                data: sample
            })
        );
        assert_eq!(from_mp4(&mp4[..mp4.len() - 4]), None);
    }

    #[test]
    fn an_odd_picture_with_alpha_and_exif_is_cropped_and_linked() {
        let (colour, alpha) = (still(7), still(9));
        let heic = write(&Heic {
            width: 1001,
            height: 625,
            colour: &colour,
            alpha: Some(&alpha),
            icc: None,
            exif: Some(b"Exif\0\0MM"),
        });
        assert_eq!(&heic[4..12], b"ftypheic");
        let meta = &child(&heic, b"meta").unwrap()[4..];
        assert_eq!(child(meta, b"pitm").unwrap(), [0, 0, 0, 0, 0, 1]);
        // Each extent in iloc points at its item's bytes.
        let iloc = &child(meta, b"iloc").unwrap()[8..];
        let at = |i: usize, o: usize| be32(iloc, i * 14 + 6 + o).unwrap() as usize;
        let item = |i: usize| &heic[at(i, 0)..at(i, 0) + at(i, 4)];
        assert_eq!((item(0), item(1)), (&colour.data[..], &alpha.data[..]));
        assert_eq!(item(2), b"\0\0\0\x06Exif\0\0MM");
        let iref = &child(meta, b"iref").unwrap()[4..];
        assert_eq!(child(iref, b"auxl").unwrap(), [0, 2, 0, 1, 0, 1]);
        assert_eq!(child(iref, b"cdsc").unwrap(), [0, 3, 0, 1, 0, 1]);
        let ispe = path(meta, &[(b"iprp", 0), (b"ipco", 0), (b"ispe", 4)]);
        assert_eq!(ispe, [1002u32.to_be_bytes(), 626u32.to_be_bytes()].concat());
        let clap = path(meta, &[(b"iprp", 0), (b"ipco", 0), (b"clap", 0)]);
        let clap: Vec<i32> = clap
            .chunks(4)
            .map(|c| i32::from_be_bytes(c.try_into().unwrap()))
            .collect();
        assert_eq!(clap, [1001, 1, 625, 1, -1, 2, -1, 2]);
        // colour: hvcC!, ispe, colr, pixi, clap!; alpha: its hvcC!, ispe, auxC!, pixi, clap!
        let ipma = path(meta, &[(b"iprp", 0), (b"ipma", 0)]);
        assert_eq!(
            &ipma[4..],
            [
                0, 0, 0, 2, 0, 1, 5, 0x81, 2, 3, 4, 0x85, 0, 2, 5, 0x86, 2, 0x87, 8, 0x85
            ]
        );
    }

    #[test]
    fn an_even_opaque_picture_needs_no_crop_or_links() {
        let colour = still(1);
        let icc = [9u8; 4];
        let heic = write(&Heic {
            width: 640,
            height: 480,
            colour: &colour,
            alpha: None,
            icc: Some(&icc),
            exif: None,
        });
        let meta = &child(&heic, b"meta").unwrap()[4..];
        assert!(child(meta, b"iref").is_none());
        let ipco = path(meta, &[(b"iprp", 0), (b"ipco", 0)]);
        assert!(child(ipco, b"clap").is_none());
        // Without an sRGB promise, the profile says what the colours are.
        assert_eq!(&child(ipco, b"colr").unwrap()[..8], b"nclx\0\x02\0\x02");
        let ipma = path(meta, &[(b"iprp", 0), (b"ipma", 0)]);
        assert_eq!(&ipma[4..], [0, 0, 0, 1, 0, 1, 5, 0x81, 2, 3, 4, 5]);
        assert_eq!(
            heic.len() - colour.data.len(),
            heic.windows(6).position(|w| w == colour.data).unwrap()
        );
    }
}
