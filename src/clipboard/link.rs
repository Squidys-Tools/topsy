//! A copied link's preview: its page's title, description and picture, read from its Open
//! Graph tags (`model::link_preview`) once, when the link is first selected, and kept in
//! `previews\` beside the history as `<hash>.json` and `<hash>.png`. Only links that are safe to
//! open unasked are fetched, through the same vetted https as the site icons.

use std::collections::HashSet;
use std::io::Cursor;
use std::path::Path;
use std::sync::{Arc, Mutex};

use gpui::RenderImage;
use image::{ImageReader, Limits};
use serde::{Deserialize, Serialize};

use super::favicon::{MAX_PAGE, follow, render};
use crate::log;
use model::favicon::resolve;
use model::link_preview::{fetchable, page_preview};

/// A picture bigger than this is not a preview.
const MAX_IMAGE: usize = 4 << 20;
/// Kept pictures are cut to the preview's shape and this wide: sharp across the card at 200%.
pub const ASPECT: f32 = 2.4;
const WIDTH: u32 = 824;

/// Links already asked this run, so a link selected twice isn't fetched twice.
static ASKED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub struct LinkPreview {
    pub title: String,
    pub description: Option<String>,
    pub image: Option<Arc<RenderImage>>,
}

/// What is kept of a page; no title means it has no preview worth showing.
#[derive(Default, Serialize, Deserialize)]
struct Kept {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

/// `link`'s preview, from `cache` (the path without its extension) or else from its page.
/// Blocks on the network: call it from a worker thread.
pub fn load(cache: &Path, link: &str) -> Option<LinkPreview> {
    let json = cache.with_extension("json");
    let png = cache.with_extension("png");
    let kept: Kept = match std::fs::read(&json) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => {
            let first = ASKED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_default()
                .insert(link.to_string());
            if !first {
                return None;
            }
            let (kept, picture) = download(link)?;
            log(&format!("clipboard preview {}", kept.title.is_some()));
            keep(cache, &kept, picture.as_deref());
            kept
        }
    };
    Some(LinkPreview {
        title: kept.title?,
        description: kept.description,
        image: std::fs::read(&png).ok().and_then(|png| render(&png)),
    })
}

/// The page's preview and its picture as a PNG. An empty one when the link mustn't be fetched
/// or the page has none; `None` when it couldn't be reached (asked again next run).
fn download(link: &str) -> Option<(Kept, Option<Vec<u8>>)> {
    let Some((host, path)) = fetchable(link) else {
        return Some((Kept::default(), None));
    };
    let Some((host, page)) = follow(&host, &path, MAX_PAGE)? else {
        return Some((Kept::default(), None));
    };
    let preview = page_preview(&String::from_utf8_lossy(&page));
    let picture = (preview.image.as_deref())
        .and_then(|href| resolve(href, &host))
        .and_then(|(host, path)| follow(&host, &path, MAX_IMAGE).flatten())
        .and_then(|(_, bytes)| shape(&bytes));
    let kept = Kept {
        title: preview.title,
        description: preview.description,
    };
    Some((kept, picture))
}

/// Through temp files: a half-written preview would read as none, for good.
fn keep(cache: &Path, kept: &Kept, picture: Option<&[u8]>) {
    let write = |path: &Path, bytes: &[u8]| {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path))
    };
    let saved = (cache.parent())
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| match picture {
            Some(png) => write(&cache.with_extension("png"), png),
            None => Ok(()),
        })
        .and_then(|()| serde_json::to_vec(kept).map_err(std::io::Error::other))
        .and_then(|json| write(&cache.with_extension("json"), &json));
    if let Err(err) = saved {
        eprintln!("topsy: could not keep a link's preview: {err}");
    }
}

/// Decode the page's picture under tight limits, cut it to the preview's shape about its
/// centre and keep it as a PNG. Cut here because GPUI rounds a covering image's overflowing
/// bounds, not the box it is clipped to.
fn shape(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 << 20);
    reader.limits(limits);
    let mut image = reader.decode().ok()?;
    let (w, h) = (image.width() as f32, image.height() as f32);
    let (cut_w, cut_h) = if w / h > ASPECT {
        (h * ASPECT, h)
    } else {
        (w, w / ASPECT)
    };
    let image = image.crop(
        ((w - cut_w) / 2.0) as u32,
        ((h - cut_h) / 2.0) as u32,
        cut_w as u32,
        cut_h as u32,
    );
    let image = if image.width() > WIDTH {
        image.resize(WIDTH, u32::MAX, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let mut png = Vec::new();
    image
        .into_rgba8()
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}
