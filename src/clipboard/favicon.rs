//! A link card's site icon: the site's `/favicon.ico` or the icon its home page names, asked once per host and kept as a small
//! PNG in `icons\` beside the history. An empty file there means the site has none. Only
//! public names are asked (see `model::favicon`), and only when a link card is shown.

use std::collections::HashSet;
use std::io::Cursor;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use gpui::RenderImage;
use image::{Frame, ImageReader, Limits};

use crate::log;
use crate::model::favicon::{page_icon, public_ip, resolve};
use crate::win32::http::{self, Response};

/// Kept icons are at most this many pixels square.
const SIZE: u32 = 64;
/// A favicon bigger than this is not one.
const MAX_BYTES: usize = 512 * 1024;
/// How much of a home page is read for its icon links, which are in its head.
const MAX_PAGE: usize = 256 * 1024;
const MAX_REDIRECTS: usize = 3;

/// Hosts already asked this run, so a strip opened twice doesn't ask twice.
static ASKED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// The icon for `host`, from `cache` or else from the site. Blocks on the network: call it from
/// a worker thread.
pub fn load(cache: &Path, host: &str) -> Option<Arc<RenderImage>> {
    let png = match std::fs::read(cache) {
        Ok(png) => png,
        Err(_) => {
            let first = ASKED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_default()
                .insert(host.to_string());
            if !first {
                return None;
            }
            let png = download(host)?;
            log(&format!("clipboard icon {host} {} bytes", png.len()));
            let saved = cache
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(cache, &png));
            if let Err(err) = saved {
                eprintln!("topsy: could not keep {host}'s icon: {err}");
            }
            png
        }
    };
    let mut image = image::load_from_memory(&png).ok()?.into_rgba8();
    // GPUI's images are BGRA.
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new(vec![Frame::new(image)])))
}

/// The site's icon as a small PNG: its `/favicon.ico`, else the icon its home page names. Empty
/// when the site answered without one, `None` when it couldn't be reached (asked again next run).
fn download(host: &str) -> Option<Vec<u8>> {
    match follow(host, "/favicon.ico", MAX_BYTES)? {
        Some((_, bytes)) if let Some(png) = shrink(&bytes) => return Some(png),
        _ => {}
    }
    let Some((home, page)) = follow(host, "/", MAX_PAGE)? else {
        return Some(Vec::new());
    };
    let page = String::from_utf8_lossy(&page);
    let Some((host, path)) = page_icon(&page).and_then(|href| resolve(href, &home)) else {
        return Some(Vec::new());
    };
    let icon = follow(&host, &path, MAX_BYTES)?;
    Some(
        icon.and_then(|(_, bytes)| shrink(&bytes))
            .unwrap_or_default(),
    )
}

/// GET `https://{host}{path}`, following redirects that stay public: the host that answered and
/// the body, `Some(None)` for no answer worth having, `None` when the network failed.
fn follow(host: &str, path: &str, limit: usize) -> Option<Option<(String, Vec<u8>)>> {
    let (mut host, mut path) = (host.to_string(), path.to_string());
    for _ in 0..=MAX_REDIRECTS {
        // A public name can still point into the local network.
        let addresses = (host.as_str(), 443).to_socket_addrs().ok()?;
        if !addresses.into_iter().all(|a| public_ip(a.ip())) {
            return Some(None);
        }
        match http::get(&host, &path, limit) {
            Response::Body(bytes) => return Some(Some((host, bytes))),
            Response::Redirect(location) => match resolve(&location, &host) {
                Some(next) => (host, path) = next,
                None => return Some(None),
            },
            Response::Missing => return Some(None),
            Response::Failed => return None,
        }
    }
    Some(None)
}

/// Decode what the site sent (an ICO, a PNG) under tight limits and keep it as a small PNG.
fn shrink(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(1024);
    limits.max_image_height = Some(1024);
    limits.max_alloc = Some(16 << 20);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    let image = if image.width() > SIZE || image.height() > SIZE {
        image.thumbnail(SIZE, SIZE)
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
