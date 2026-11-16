//! Combine into PDF: one page per picture, and every page of each PDF, in the order given.

use std::io;
use std::path::{Path, PathBuf};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};

use super::{load, save};
use crate::model::image_ops::{Format, Kind, is_lossless_webp};
use crate::model::jpeg;
use crate::model::magick::{self, Job, Metadata};
use crate::model::orientation::Orientation;
use crate::model::pdf::{PageSize, draw, place};
use crate::transform::Steps;
use crate::transform::image::{Kit, Scratch};

/// One file to combine, read and sorted by what it takes to put it in.
pub enum Part {
    Pdf(Vec<u8>),
    /// A JPEG that goes in as it is.
    Jpeg(Embeddable),
    /// Anything else: ImageMagick makes it embeddable first.
    Other(PathBuf, Kind),
}

pub struct Embeddable {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    components: u8,
    icc: Option<Vec<u8>>,
}

impl Part {
    pub fn read(src: &Path) -> io::Result<Part> {
        let bytes = std::fs::read(src)?;
        Ok(match Kind::sniff(&bytes) {
            Some(Kind::Pdf) => Part::Pdf(bytes),
            Some(Kind::Jpeg) => match embeddable_jpeg(&bytes) {
                Some(e) => Part::Jpeg(e),
                None => Part::Other(src.to_path_buf(), Kind::Jpeg),
            },
            Some(kind) => Part::Other(src.to_path_buf(), kind),
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "topsy can't put this kind of file in a PDF",
                ));
            }
        })
    }

    pub fn needs_magick(&self) -> bool {
        matches!(self, Part::Other(..))
    }
}

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

    pub fn add(
        &mut self,
        part: Part,
        size: PageSize,
        kit: Option<&Kit>,
        steps: &Steps,
    ) -> io::Result<()> {
        match part {
            Part::Pdf(bytes) => self.add_pdf(&bytes),
            Part::Jpeg(e) => {
                self.add_jpeg(e, size);
                Ok(())
            }
            Part::Other(src, kind) => {
                let kit = kit.ok_or_else(|| io::Error::other("ImageMagick isn't here"))?;
                self.add_converted(&src, kind, size, kit, steps)
            }
        }
    }

    /// A picture PDF can't carry as it is. ImageMagick turns it upright and writes a quality 90
    /// JPEG of a lossy, opaque one, or a PNG of the rest, whose pixels go in losslessly (Flate)
    /// with any transparency as a soft mask.
    fn add_converted(
        &mut self,
        src: &Path,
        kind: Kind,
        size: PageSize,
        kit: &Kit,
        steps: &Steps,
    ) -> io::Result<()> {
        let lossy = match kind {
            Kind::Jpeg | Kind::Heic | Kind::Avif => true,
            Kind::Webp => !is_lossless_webp(&std::fs::read(src)?),
            _ => false,
        };
        let as_jpeg = lossy && (kind == Kind::Jpeg || kit.opaque(src, steps)?);
        let format = if as_jpeg { Format::Jpeg } else { Format::Png };
        let scratch = Scratch::new(format.extension());
        let job = Job {
            quality: as_jpeg.then_some(90),
            metadata: Metadata::Remove,
            ..Job::new(format)
        };
        let path = |p: &Path| p.to_string_lossy().into_owned();
        kit.magick(&magick::args(&job, &path(src), &path(&scratch.0)), steps)?;
        let bytes = std::fs::read(&scratch.0)?;
        if as_jpeg {
            let e = embeddable_jpeg(&bytes).ok_or_else(super::unreadable)?;
            self.add_jpeg(e, size);
            return Ok(());
        }
        let mut decoder = image::codecs::png::PngDecoder::new(io::Cursor::new(&bytes))
            .map_err(|_| super::unreadable())?;
        let icc = image::ImageDecoder::icc_profile(&mut decoder)
            .ok()
            .flatten();
        let picture = image::DynamicImage::from_decoder(decoder)
            .map_err(|_| super::unreadable())?
            .into_rgba8();
        let (w, h) = picture.dimensions();
        let rgb: Vec<u8> = picture
            .pixels()
            .flat_map(|p| [p.0[0], p.0[1], p.0[2]])
            .collect();
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
        let mut image = flate_stream(data, w, h, n);
        self.set_icc(&mut image, icc.as_deref(), n as i64);
        if picture.pixels().any(|p| p.0[3] < 255) {
            let alpha: Vec<u8> = picture.pixels().map(|p| p.0[3]).collect();
            let mask = self.doc.add_object(flate_stream(alpha, w, h, 1));
            image.dict.set("SMask", mask);
        }
        self.add_page(image, w, h, size);
        Ok(())
    }

    fn add_jpeg(&mut self, e: Embeddable, size: PageSize) {
        let space = if e.components == 1 {
            "DeviceGray"
        } else {
            "DeviceRGB"
        };
        let mut image =
            Stream::new(image_dict(e.width, e.height, space), e.bytes).with_compression(false);
        image.dict.set("Filter", "DCTDecode");
        self.set_icc(&mut image, e.icc.as_deref(), i64::from(e.components));
        self.add_page(image, e.width, e.height, size);
    }

    fn set_icc(&mut self, image: &mut Stream, icc: Option<&[u8]>, n: i64) {
        let Some(icc) = icc.filter(|icc| icc_components(icc) == Some(n)) else {
            return;
        };
        let mut profile = Stream::new(dictionary! { "N" => n }, icc.to_vec());
        let _ = profile.compress();
        let id = self.doc.add_object(profile);
        image.dict.set(
            "ColorSpace",
            vec![Object::Name(b"ICCBased".to_vec()), Object::Reference(id)],
        );
    }

    fn add_page(&mut self, image: Stream, w: u32, h: u32, size: PageSize) {
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

/// The JPEG's own bytes, upright and without EXIF, when PDF can show them as they are: 8-bit
/// grey or colour (not CMYK, whose inverted Adobe flavour PDF readers disagree on), and either
/// already upright or turnable without loss.
fn embeddable_jpeg(original: &[u8]) -> Option<Embeddable> {
    let o = jpeg::orientation(original);
    let turned;
    let bytes = if o == Orientation::Normal {
        original
    } else {
        turned = jpeg::rotate_lossless(original, o).ok()??;
        &turned[..]
    };
    let segments = jpeg::markers::segments(bytes).ok()?;
    let icc = jpeg::markers::icc(&segments);
    let sof = segments
        .iter()
        .find(|seg| matches!(seg.marker, 0xC0..=0xC2))?;
    let d = sof.data;
    let (depth, components) = (*d.first()?, *d.get(5)?);
    let height = u32::from(u16::from_be_bytes([*d.get(1)?, *d.get(2)?]));
    let width = u32::from(u16::from_be_bytes([*d.get(3)?, *d.get(4)?]));
    if depth != 8 || !matches!(components, 1 | 3) || width == 0 || height == 0 {
        return None;
    }
    Some(Embeddable {
        bytes: jpeg::markers::strip_metadata(bytes).ok()?,
        width,
        height,
        components,
        icc,
    })
}

/// 8-bit samples, `n` to a pixel, deflated.
fn flate_stream(samples: Vec<u8>, w: u32, h: u32, n: usize) -> Stream {
    let space = if n == 1 { "DeviceGray" } else { "DeviceRGB" };
    let mut s = Stream::new(image_dict(w, h, space), samples);
    let _ = s.compress();
    s
}

fn image_dict(w: u32, h: u32, space: &str) -> Dictionary {
    dictionary! {
        "Type" => "XObject",
        "Subtype" => "Image",
        "Width" => w,
        "Height" => h,
        "ColorSpace" => space,
        "BitsPerComponent" => 8,
    }
}

/// How many components an ICC profile describes, from its header.
fn icc_components(icc: &[u8]) -> Option<i64> {
    match icc.get(16..20)? {
        b"RGB " => Some(3),
        b"GRAY" => Some(1),
        _ => None,
    }
}
