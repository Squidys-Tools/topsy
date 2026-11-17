//! Compress PDF: pictures re-encoded as JPEG by ImageMagick and scaled down to what their page
//! shows, unused objects dropped, and the rest packed into object streams.

use std::collections::HashSet;
use std::io;

use lopdf::{Document, Object, ObjectId, Stream};

use super::place::{media_box, picture_sizes};
use crate::model::magick::pdf_image_args;
use crate::model::pdf::{Preset, downsample, worth_replacing};
use crate::model::pdf_image::{adobe_cmyk, through_palette};
use crate::transform::Steps;
use crate::transform::image::{Kit, Scratch};

/// The pictures in `doc`, without the masks that belong to them.
fn pictures(doc: &Document) -> Vec<ObjectId> {
    let masks: HashSet<ObjectId> = doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .flat_map(|s| [s.dict.get(b"SMask"), s.dict.get(b"Mask")])
        .filter_map(|m| m.ok()?.as_reference().ok())
        .collect();
    doc.objects
        .iter()
        .filter(|(id, o)| {
            !masks.contains(id)
                && o.as_stream().is_ok_and(|s| {
                    s.dict
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .is_ok_and(|n| n == b"Image")
                })
        })
        .map(|(id, _)| *id)
        .collect()
}

pub fn has_pictures(doc: &Document) -> bool {
    !pictures(doc).is_empty()
}

/// Applies `preset` to every picture in `doc`, reporting progress from `from` to `to`. Without
/// `kit` (a PDF with no pictures) only the structure is packed.
pub fn shrink(
    doc: &mut Document,
    preset: Preset,
    kit: Option<&Kit>,
    steps: &Steps,
    (from, to): (f32, f32),
) -> io::Result<()> {
    let sizes = picture_sizes(doc);
    let fallback = doc
        .page_iter()
        .filter_map(|page| media_box(doc, page))
        .fold(
            (612.0, 792.0),
            |a: (f64, f64), b| if b.0 * b.1 > a.0 * a.1 { b } else { a },
        );
    let images = if kit.is_some() {
        pictures(doc)
    } else {
        Vec::new()
    };
    for (i, id) in images.iter().enumerate() {
        steps.at(from + (to - from) * i as f32 / images.len().max(1) as f32)?;
        let colour = match doc.get_object(*id).and_then(Object::as_stream) {
            Ok(stream) => colour(doc, stream),
            Err(_) => continue,
        };
        let (Some(colour), Some(kit)) = (colour, kit) else {
            continue;
        };
        let shown = sizes.get(id).copied().unwrap_or(fallback);
        let Ok(Object::Stream(stream)) = doc.get_object_mut(*id) else {
            continue;
        };
        if let Some((jpeg, w, h)) = reencode(stream, &colour, shown, preset, kit, steps)? {
            stream.dict.set("Width", w);
            stream.dict.set("Height", h);
            stream.dict.set("Filter", "DCTDecode");
            stream.dict.set("BitsPerComponent", 8);
            stream.dict.remove(b"DecodeParms");
            match colour.samples {
                Samples::Indexed { .. } => stream.dict.set("ColorSpace", colour.space),
                Samples::Cmyk => stream.dict.set(
                    "Decode",
                    INVERTED.iter().map(|&v| v.into()).collect::<Vec<Object>>(),
                ),
                Samples::Plain(_) => {}
            }
            stream.set_content(jpeg);
            stream.allows_compression = false;
        }
    }
    doc.prune_objects();
    doc.delete_zero_length_streams();
    doc.compress();
    Ok(())
}

/// CMYK's `/Decode`, which Adobe's JPEGs (and so ImageMagick's) need.
const INVERTED: [i64; 8] = [1, 0, 1, 0, 1, 0, 1, 0];

/// How a picture's samples read.
enum Samples {
    /// 8-bit grey (1) or RGB (3), as they are.
    Plain(u8),
    /// 8-bit indices into `palette`, `base` bytes (1 grey, 3 RGB) an entry.
    Indexed { base: u8, palette: Vec<u8> },
    /// 8-bit CMYK, a JPEG written the Adobe way.
    Cmyk,
}

struct Colour {
    samples: Samples,
    /// The colour space the picture is in, or for an indexed one the space its palette is in.
    space: Object,
}

fn device(doc: &Document, space: &Object) -> Option<(u8, Object)> {
    let (_, space) = doc.dereference(space).ok()?;
    let n = match space {
        Object::Name(n) if n == b"DeviceRGB" => 3,
        Object::Name(n) if n == b"DeviceGray" => 1,
        Object::Name(n) if n == b"DeviceCMYK" => 4,
        Object::Array(a) if a.first().and_then(|n| n.as_name().ok()) == Some(b"ICCBased") => {
            let (_, profile) = doc.dereference(a.get(1)?).ok()?;
            let n = profile
                .as_stream()
                .ok()?
                .dict
                .get(b"N")
                .ok()?
                .as_i64()
                .ok()?;
            u8::try_from(n).ok().filter(|n| matches!(n, 1 | 3 | 4))?
        }
        _ => return None,
    };
    Some((n, space.clone()))
}

/// What the picture's colours are, when it is 8-bit with nothing unusual about how it decodes.
fn colour(doc: &Document, s: &Stream) -> Option<Colour> {
    let d = &s.dict;
    let int = |k: &[u8]| d.get(k).ok().and_then(|v| v.as_i64().ok());
    // A colour key in /Mask picks out exact colours, which a JPEG no longer has.
    let keyed = d.get(b"Mask").is_ok_and(|m| m.as_array().is_ok());
    if int(b"BitsPerComponent") != Some(8)
        || d.get(b"ImageMask")
            .and_then(Object::as_bool)
            .unwrap_or(false)
        || d.has(b"SMaskInData")
        || keyed
    {
        return None;
    }
    let (_, space) = doc.dereference(d.get(b"ColorSpace").ok()?).ok()?;
    if let Object::Array(a) = space
        && a.first().and_then(|n| n.as_name().ok()) == Some(b"Indexed")
    {
        if d.has(b"Decode") {
            return None;
        }
        let (base, space) = device(doc, a.get(1)?).filter(|(n, _)| *n != 4)?;
        let palette = match doc.dereference(a.get(3)?).ok()?.1 {
            Object::String(bytes, _) => bytes.clone(),
            Object::Stream(s) => s.decompressed_content().ok()?,
            _ => return None,
        };
        return Some(Colour {
            samples: Samples::Indexed { base, palette },
            space,
        });
    }
    let (n, space) = device(doc, space)?;
    // Only CMYK may be inverted by /Decode, and `reencode` checks it is the Adobe way.
    let samples = match n {
        4 => Samples::Cmyk,
        _ if d.has(b"Decode") => return None,
        n => Samples::Plain(n),
    };
    Some(Colour { samples, space })
}

/// What ImageMagick is given for a picture, and the extension that tells it how to read that: a
/// JPEG as it is, raw samples as a PPM or PGM. `None` for a picture that is left alone.
fn source(s: &Stream, colour: &Colour, (w, h): (u32, u32)) -> Option<(Vec<u8>, &'static str)> {
    let filters = s.filters().unwrap_or_default();
    if matches!(filters.as_slice(), [b"DCTDecode"]) {
        let jpeg = match colour.samples {
            Samples::Plain(_) => true,
            Samples::Cmyk => {
                let decode: Option<Vec<f64>> = s.dict.get(b"Decode").ok().and_then(|d| {
                    let numbers = d.as_array().ok()?;
                    numbers
                        .iter()
                        .map(|n| n.as_float().ok().map(f64::from))
                        .collect()
                });
                adobe_cmyk(decode.as_deref(), &s.content)
            }
            Samples::Indexed { .. } => false,
        };
        return jpeg.then(|| (s.content.clone(), "jpg"));
    }
    if !(filters.is_empty() || matches!(filters.as_slice(), [b"FlateDecode"])) {
        return None;
    }
    let (n, indexed) = match &colour.samples {
        Samples::Plain(n) => (usize::from(*n), None),
        Samples::Indexed { base, palette } => (usize::from(*base), Some(palette)),
        Samples::Cmyk => return None,
    };
    let stored = w as usize * h as usize * if indexed.is_some() { 1 } else { n };
    // A little over, so a stream with a stray byte is read and then refused for its length.
    let raw = s
        .decompressed_content_with_limit(stored + h as usize + 1)
        .ok()?;
    if raw.len() != stored {
        return None;
    }
    let raw = match indexed {
        Some(palette) => through_palette(&raw, palette, n)?,
        None => raw,
    };
    let magic = if n == 1 { "P5" } else { "P6" };
    let mut pnm = format!(
        "{magic}
{w} {h}
255
"
    )
    .into_bytes();
    pnm.extend_from_slice(&raw);
    Some((pnm, if n == 1 { "pgm" } else { "ppm" }))
}

/// The picture as a JPEG at `preset`, scaled for what it shows, when that is worth it.
fn reencode(
    s: &Stream,
    colour: &Colour,
    shown: (f64, f64),
    preset: Preset,
    kit: &Kit,
    steps: &Steps,
) -> io::Result<Option<(Vec<u8>, u32, u32)>> {
    let int = |k: &[u8]| s.dict.get(k).ok()?.as_i64().ok()?.try_into().ok();
    let (Some(w), Some(h)) = (int(b"Width"), int(b"Height")) else {
        return Ok(None);
    };
    let (w, h): (u32, u32) = (w, h);
    if w < 16 || h < 16 {
        return Ok(None);
    }
    let Some((input, ext)) = source(s, colour, (w, h)) else {
        return Ok(None);
    };
    let n = match &colour.samples {
        Samples::Plain(n) => *n,
        Samples::Indexed { base, .. } => *base,
        Samples::Cmyk => 4,
    };
    let target = downsample(w, h, shown, preset.dpi);
    let (src, out) = (Scratch::new(ext), Scratch::new("jpg"));
    std::fs::write(&src.0, &input)?;
    let (tw, th) = target.unwrap_or((w, h));
    let path = |p: &std::path::Path| p.to_string_lossy().into_owned();
    let args = pdf_image_args(&path(&src.0), n, (tw, th), preset.quality, &path(&out.0));
    match kit.magick(&args, steps) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::Interrupted => return Err(e),
        // A picture ImageMagick can't read stays as it is.
        Err(_) => return Ok(None),
    }
    let bytes = std::fs::read(&out.0)?;
    Ok(worth_replacing(s.content.len(), bytes.len(), target.is_some()).then_some((bytes, tw, th)))
}

#[cfg(test)]
mod tests;
