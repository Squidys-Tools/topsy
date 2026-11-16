//! Compress PDF: pictures re-encoded as JPEG by ImageMagick and scaled down to what their page
//! can show, streams deflated, unused objects dropped, and the rest packed into object streams.

use std::collections::{HashMap, HashSet};
use std::io;

use lopdf::{Document, Object, ObjectId, Stream};

use crate::model::magick::pdf_image_args;
use crate::model::pdf::{Preset, downsample, worth_replacing};
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
    let pages = page_sizes(doc);
    let fallback =
        pages.values().copied().fold(
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
        let components = match doc.get_object(*id).and_then(Object::as_stream) {
            Ok(stream) => components(doc, stream),
            Err(_) => continue,
        };
        let (Some(n), Some(kit)) = (components, kit) else {
            continue;
        };
        let page = pages.get(id).copied().unwrap_or(fallback);
        let Ok(Object::Stream(stream)) = doc.get_object_mut(*id) else {
            continue;
        };
        if let Some((jpeg, w, h)) = reencode(stream, n, page, preset, kit, steps)? {
            stream.dict.set("Width", w);
            stream.dict.set("Height", h);
            stream.dict.set("Filter", "DCTDecode");
            stream.dict.set("BitsPerComponent", 8);
            stream.dict.remove(b"DecodeParms");
            stream.set_content(jpeg);
            stream.allows_compression = false;
        }
    }
    doc.prune_objects();
    doc.delete_zero_length_streams();
    doc.compress();
    Ok(())
}

/// For every picture a page draws (directly or through a form), the largest page it is on.
fn page_sizes(doc: &Document) -> HashMap<ObjectId, (f64, f64)> {
    let mut sizes: HashMap<ObjectId, (f64, f64)> = HashMap::new();
    for page in doc.page_iter() {
        let size = media_box(doc, page).unwrap_or((612.0, 792.0));
        let Ok((resources, inherited)) = doc.get_page_resources(page) else {
            continue;
        };
        let mut stack: Vec<&lopdf::Dictionary> = resources.into_iter().collect();
        stack.extend(
            inherited
                .iter()
                .filter_map(|id| doc.get_dictionary(*id).ok()),
        );
        let mut seen = HashSet::new();
        while let Some(res) = stack.pop() {
            let Ok(xobjects) = res
                .get(b"XObject")
                .and_then(|x| doc.dereference(x))
                .and_then(|(_, x)| x.as_dict())
            else {
                continue;
            };
            for (_, x) in xobjects.iter() {
                let Ok(id) = x.as_reference() else { continue };
                if !seen.insert(id) || seen.len() > 10_000 {
                    continue;
                }
                let entry = sizes.entry(id).or_insert(size);
                if size.0 * size.1 > entry.0 * entry.1 {
                    *entry = size;
                }
                if let Ok(form) = doc.get_object(id).and_then(Object::as_stream)
                    && let Ok(inner) = form
                        .dict
                        .get(b"Resources")
                        .and_then(|r| doc.dereference(r))
                        .and_then(|(_, r)| r.as_dict())
                {
                    stack.push(inner);
                }
            }
        }
    }
    sizes
}

fn media_box(doc: &Document, page: ObjectId) -> Option<(f64, f64)> {
    let mut node = doc.get_dictionary(page).ok()?;
    for _ in 0..64 {
        if let Ok(b) = node
            .get(b"MediaBox")
            .and_then(|b| doc.dereference(b))
            .and_then(|(_, b)| b.as_array())
        {
            let v: Vec<f64> = b
                .iter()
                .filter_map(|n| n.as_float().ok().map(f64::from))
                .collect();
            return (v.len() == 4).then(|| ((v[2] - v[0]).abs(), (v[3] - v[1]).abs()));
        }
        node = doc
            .get_dictionary(node.get(b"Parent").ok()?.as_reference().ok()?)
            .ok()?;
    }
    None
}

/// 1 or 3 when the picture is 8-bit grey or RGB with nothing unusual about how it decodes.
fn components(doc: &Document, s: &Stream) -> Option<u8> {
    let d = &s.dict;
    let int = |k: &[u8]| d.get(k).ok().and_then(|v| v.as_i64().ok());
    if int(b"BitsPerComponent") != Some(8)
        || d.get(b"ImageMask")
            .and_then(Object::as_bool)
            .unwrap_or(false)
        || d.has(b"Decode")
        || d.has(b"SMaskInData")
    {
        return None;
    }
    let (_, space) = doc.dereference(d.get(b"ColorSpace").ok()?).ok()?;
    match space {
        Object::Name(n) if n == b"DeviceRGB" => Some(3),
        Object::Name(n) if n == b"DeviceGray" => Some(1),
        Object::Array(a) if a.first().and_then(|n| n.as_name().ok()) == Some(b"ICCBased") => {
            let (_, profile) = doc.dereference(a.get(1)?).ok()?;
            match profile
                .as_stream()
                .ok()?
                .dict
                .get(b"N")
                .ok()?
                .as_i64()
                .ok()?
            {
                1 => Some(1),
                3 => Some(3),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The picture as a JPEG at `preset`, scaled for `page`, when that is worth it. ImageMagick
/// reads a JPEG as it is and raw samples as a PPM or PGM.
fn reencode(
    s: &Stream,
    n: u8,
    page: (f64, f64),
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
    let target = downsample(w, h, page, preset.dpi);
    let filters = s.filters().unwrap_or_default();
    let (input, ext) = match filters.as_slice() {
        [b"DCTDecode"] => (s.content.clone(), "jpg"),
        [] | [b"FlateDecode"] => {
            let expected = w as usize * h as usize * usize::from(n);
            let Ok(raw) = s.decompressed_content_with_limit(expected + h as usize + 1) else {
                return Ok(None);
            };
            if raw.len() != expected {
                return Ok(None);
            }
            let magic = if n == 1 { "P5" } else { "P6" };
            let mut pnm = format!("{magic}\n{w} {h}\n255\n").into_bytes();
            pnm.extend_from_slice(&raw);
            (pnm, if n == 1 { "pgm" } else { "ppm" })
        }
        _ => return Ok(None),
    };
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
