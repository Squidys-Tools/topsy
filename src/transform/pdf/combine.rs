//! Combine into PDF: one page per picture, and every page of each PDF, in the order given.

use std::io;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};

use super::{load, save};
use crate::model::image_ops::Kind;
use crate::model::jpeg::{self, encode as jpeg_encode};
use crate::model::orientation::Orientation;
use crate::model::pdf::{PageSize, draw, place};
use crate::model::png_opt::deflate_predicted;
use crate::transform::image::source::Source;

pub struct Builder {
    doc: Document,
    pages: ObjectId,
    kids: Vec<Object>,
}

/// Inherited page attributes that have to move onto a page when it changes parent.
const INHERITED: [&[u8]; 4] = [b"MediaBox", b"CropBox", b"Resources", b"Rotate"];

impl Builder {
    pub fn new() -> Self {
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        Self {
            doc,
            pages,
            kids: Vec::new(),
        }
    }

    pub fn pages(&self) -> usize {
        self.kids.len()
    }

    /// A page showing `s`. A JPEG goes in as it is (DCTDecode) when it can; one that can't, or a
    /// picture from another lossy format, goes in as a quality 90 JPEG; anything else stays
    /// lossless (Flate), with its transparency as a soft mask.
    pub fn add_picture(&mut self, s: &Source, size: PageSize) -> io::Result<()> {
        let (w, h) = s.image.dimensions();
        let mut image = match embeddable_jpeg(s) {
            Some((bytes, components)) => jpeg_stream(bytes, w, h, components),
            None if is_lossy(s) => {
                let rgb = straight_rgb(s);
                let px = jpeg_encode::Pixels {
                    width: w as usize,
                    height: h as usize,
                    channels: 3,
                    data: &rgb,
                };
                let bytes = jpeg_encode::encode(
                    &px,
                    90,
                    &jpeg_encode::Metadata {
                        exif: None,
                        icc: None,
                    },
                )?;
                jpeg_stream(bytes, w, h, 3)
            }
            None => {
                let rgb = straight_rgb(s);
                let grey = rgb
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .all(|p| p[0] == p[1] && p[1] == p[2]);
                let (data, n) = if grey {
                    (rgb.iter().step_by(3).copied().collect(), 1)
                } else {
                    (rgb, 3)
                };
                flate_stream(&data, w, h, n)
            }
        };
        let components = image
            .dict
            .get(b"ColorSpace")
            .ok()
            .and_then(|c| c.as_name().ok())
            .map(|n| match n {
                b"DeviceGray" => 1,
                _ => 3,
            });
        if let (Some(icc), Some(n)) = (&s.icc, components)
            && icc_components(icc) == Some(n)
        {
            let mut profile = Stream::new(dictionary! { "N" => n }, icc.clone());
            let _ = profile.compress();
            let id = self.doc.add_object(profile);
            image.dict.set(
                "ColorSpace",
                vec![Object::Name(b"ICCBased".to_vec()), Object::Reference(id)],
            );
        }
        if s.image.pixels().any(|p| p.0[3] < 255) {
            let alpha: Vec<u8> = s.image.pixels().map(|p| p.0[3]).collect();
            let mask = self.doc.add_object(flate_stream(&alpha, w, h, 1));
            image.dict.set("SMask", mask);
        }
        let image = self.doc.add_object(image);
        let at = place(w, h, size);
        let mut content = Stream::new(dictionary! {}, draw(&at));
        let _ = content.compress();
        let content = self.doc.add_object(content);
        let page = self.doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => self.pages,
            "MediaBox" => vec![0.into(), 0.into(), Object::Real(at.page.0 as f32), Object::Real(at.page.1 as f32)],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
            "Contents" => content,
        });
        self.kids.push(page.into());
        Ok(())
    }

    /// Every page of the PDF in `bytes`, after the pages so far.
    pub fn add_pdf(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut src = load(bytes)?;
        src.renumber_objects_with(self.doc.max_id + 1);
        let pages: Vec<ObjectId> = src.get_pages().into_values().collect();
        for &page in &pages {
            let inherited: Vec<(&[u8], Object)> = INHERITED
                .iter()
                .filter_map(|key| inherited(&src, page, key).map(|v| (*key, v)))
                .collect();
            let dict = src
                .get_dictionary_mut(page)
                .map_err(|_| super::unreadable())?;
            for (key, value) in inherited {
                if !dict.has(key) {
                    dict.set(key, value);
                }
            }
            dict.set("Parent", self.pages);
            self.kids.push(page.into());
        }
        for (id, object) in src.objects {
            let skip = matches!(&object, Object::Dictionary(d) if d.has_type(b"Catalog") || d.has_type(b"Pages"));
            if !skip {
                self.doc.objects.insert(id, object);
            }
        }
        self.doc.max_id = self.doc.max_id.max(src.max_id);
        Ok(())
    }

    pub fn finish(mut self) -> io::Result<(Vec<u8>, u32)> {
        let count = self.kids.len() as u32;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "There's nothing to put in the PDF",
            ));
        }
        self.doc.objects.insert(
            self.pages,
            dictionary! { "Type" => "Pages", "Kids" => self.kids, "Count" => count }.into(),
        );
        let catalog = self
            .doc
            .add_object(dictionary! { "Type" => "Catalog", "Pages" => self.pages });
        self.doc.trailer.set("Root", catalog);
        self.doc.prune_objects();
        Ok((save(&mut self.doc)?, count))
    }
}

/// An attribute of `page`, from the page itself or the nearest parent that has it.
pub(super) fn inherited(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Object> {
    let mut node = doc.get_dictionary(page).ok()?;
    for _ in 0..64 {
        if let Ok(v) = node.get(key) {
            return Some(v.clone());
        }
        node = doc
            .get_dictionary(node.get(b"Parent").ok()?.as_reference().ok()?)
            .ok()?;
    }
    None
}

fn is_lossy(s: &Source) -> bool {
    match s.kind {
        Kind::Jpeg | Kind::Heic | Kind::Avif => true,
        Kind::Webp => s.bytes.windows(4).find(|w| *w == b"VP8 " || *w == b"VP8L") == Some(b"VP8 "),
        _ => false,
    }
}

/// The JPEG's own bytes, upright and without EXIF, when PDF can show them as they are: 8-bit
/// grey or colour (not CMYK, whose inverted Adobe flavour PDF readers disagree on), and either
/// already upright or turnable without loss.
fn embeddable_jpeg(s: &Source) -> Option<(Vec<u8>, u8)> {
    if s.kind != Kind::Jpeg {
        return None;
    }
    let bytes = if s.orientation == Orientation::Normal {
        s.bytes.clone()
    } else {
        jpeg::rotate_lossless(&s.bytes, s.orientation).ok()??
    };
    let bytes = jpeg::markers::strip_metadata(&bytes).ok()?;
    let segments = jpeg::markers::segments(&bytes).ok()?;
    let sof = segments
        .iter()
        .find(|seg| matches!(seg.marker, 0xC0..=0xC2))?;
    let (&depth, &components) = (sof.data.first()?, sof.data.get(5)?);
    (depth == 8 && matches!(components, 1 | 3)).then_some((bytes, components))
}

fn jpeg_stream(bytes: Vec<u8>, w: u32, h: u32, components: u8) -> Stream {
    let space = if components == 1 {
        "DeviceGray"
    } else {
        "DeviceRGB"
    };
    Stream::new(image_dict(w, h, space, "DCTDecode"), bytes).with_compression(false)
}

/// 8-bit samples, `n` to a pixel, deflated with PNG prediction.
pub fn flate_stream(samples: &[u8], w: u32, h: u32, n: usize) -> Stream {
    let space = if n == 1 { "DeviceGray" } else { "DeviceRGB" };
    let mut dict = image_dict(w, h, space, "FlateDecode");
    dict.set(
        "DecodeParms",
        dictionary! { "Predictor" => 15, "Colors" => n as i64, "Columns" => w },
    );
    Stream::new(dict, deflate_predicted(samples, w as usize, n)).with_compression(false)
}

fn image_dict(w: u32, h: u32, space: &str, filter: &str) -> Dictionary {
    dictionary! {
        "Type" => "XObject",
        "Subtype" => "Image",
        "Width" => w,
        "Height" => h,
        "ColorSpace" => space,
        "BitsPerComponent" => 8,
        "Filter" => filter,
    }
}

/// The colour, unblended: a soft mask carries the transparency.
fn straight_rgb(s: &Source) -> Vec<u8> {
    s.image
        .pixels()
        .flat_map(|p| [p.0[0], p.0[1], p.0[2]])
        .collect()
}

/// How many components an ICC profile describes, from its header.
fn icc_components(icc: &[u8]) -> Option<i64> {
    match icc.get(16..20)? {
        b"RGB " => Some(3),
        b"GRAY" => Some(1),
        _ => None,
    }
}
