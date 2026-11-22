//! The shell's own picture of a file, as Explorer shows it: a thumbnail where the file has one
//! (photos, videos, documents with previews), its icon otherwise.
//!
//! Asking can read the file and wake a thumbnail handler, which takes a while for a video or a
//! cold network path, so call it from a worker thread, never the UI thread.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{App, RenderImage};
use image::{Frame, RgbaImage};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HBITMAP,
};
use windows::Win32::System::Com::COINIT_MULTITHREADED;
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
};
use windows::core::HSTRING;

use crate::log::LogErr;
use model::transform::Kind;

/// What [`resolve`] learns of a path: whether it is a folder, its size, and its picture.
pub type Resolved = (PathBuf, bool, Option<u64>, Option<Arc<RenderImage>>);

/// Whether each path is a folder, its size, and its picture about `size` physical pixels square,
/// learnt on a worker thread of its own (the shell wants COM there) and sent back one path at a
/// time.
pub fn resolve(paths: Vec<PathBuf>, size: i32) -> async_channel::Receiver<Resolved> {
    let (tx, rx) = async_channel::unbounded();
    std::thread::spawn(move || {
        crate::win32::with_com(COINIT_MULTITHREADED, || {
            for path in paths {
                let meta = path.metadata().ok();
                let is_dir = meta.as_ref().is_some_and(std::fs::Metadata::is_dir);
                let bytes = meta.map(|meta| meta.len());
                let icon = load(&path, size);
                if tx.send_blocking((path, is_dir, bytes, icon)).is_err() {
                    break;
                }
            }
        })
    });
    rx
}

/// The picture for `path`, about `size` physical pixels square. Call inside
/// [`crate::win32::with_com`].
pub fn load(path: &Path, size: i32) -> Option<Arc<RenderImage>> {
    pixels(path, size).map(render)
}

/// Like [`load`], cropped to `aspect` (width over height) about its centre, and blurred by
/// `sigma` pixels if given: drawn large, a small blurred one is a soft wash of the picture's
/// colours (the waiting Transform cards). Cropped here because GPUI rounds a covering image's
/// overflowing bounds, not the box it is clipped to.
pub fn load_cover(
    path: &Path,
    size: i32,
    aspect: f32,
    sigma: Option<f32>,
) -> Option<Arc<RenderImage>> {
    let p = pixels(path, size)?;
    let (w, h) = (p.width() as f32, p.height() as f32);
    let (cw, ch) = if w / h > aspect {
        (h * aspect, h)
    } else {
        (w, w / aspect)
    };
    let (cw, ch) = ((cw as u32).max(1), (ch as u32).max(1));
    let x = (p.width() - cw.min(p.width())) / 2;
    let y = (p.height() - ch.min(p.height())) / 2;
    let cropped = image::imageops::crop_imm(&p, x, y, cw, ch).to_image();
    Some(render(match sigma {
        Some(sigma) => image::imageops::blur(&cropped, sigma),
        None => cropped,
    }))
}

/// A PDF is drawn by pdfium: the shell's thumbnail for one comes from whatever PDF app is the
/// default, which most PCs have none of, so it would answer with a generic icon.
fn pixels(path: &Path, size: i32) -> Option<RgbaImage> {
    if Kind::of(path) == Kind::Pdf
        && let Some(page) = crate::transform::pdf::first_page(path, size)
    {
        return Some(page);
    }
    let (width, height, mut pixels) = unsafe { shell_bitmap(path, size) }?;
    straighten(&mut pixels);
    // GPUI's images are BGRA with straight alpha, whatever the buffer's type says.
    RgbaImage::from_raw(width, height, pixels)
}

fn render(pixels: RgbaImage) -> Arc<RenderImage> {
    Arc::new(RenderImage::new(vec![Frame::new(pixels)]))
}

/// Free chip pictures' textures in the window's atlas, which otherwise keeps them. Deferred:
/// inside a window's event handler GPUI has lent the window out, and `drop_image` can't reach it.
pub fn forget(icons: Vec<Arc<RenderImage>>, cx: &mut App) {
    if !icons.is_empty() {
        cx.defer(move |cx| {
            for icon in icons {
                cx.drop_image(icon, None);
            }
        });
    }
}

unsafe fn shell_bitmap(path: &Path, size: i32) -> Option<(u32, u32, Vec<u8>)> {
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None) }.ok()?;
    let bitmap =
        unsafe { factory.GetImage(SIZE { cx: size, cy: size }, SIIGBF_BIGGERSIZEOK) }.ok()?;
    let pixels = unsafe { bitmap_pixels(bitmap) };
    unsafe { DeleteObject(bitmap.into()) }.ok().log_err();
    pixels
}

/// The bitmap's pixels as top-down 32-bit BGRA.
unsafe fn bitmap_pixels(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    let mut info = BITMAP::default();
    let got = unsafe {
        GetObjectW(
            bitmap.into(),
            size_of::<BITMAP>() as i32,
            Some(&mut info as *mut BITMAP as *mut c_void),
        )
    };
    let (width, height) = (info.bmWidth, info.bmHeight.abs());
    if got == 0 || width <= 0 || height <= 0 {
        return None;
    }
    let mut header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative: rows top to bottom.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let dc = unsafe { CreateCompatibleDC(None) };
    let lines = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr() as *mut c_void),
            &mut header,
            DIB_RGB_COLORS,
        )
    };
    unsafe { DeleteDC(dc) }.ok().log_err();
    (lines == height).then_some((width as u32, height as u32, pixels))
}

/// The shell hands back premultiplied alpha for icons, and no alpha at all (every byte 0) for
/// most photo and video thumbnails; GPUI wants straight alpha.
fn straighten(bgra: &mut [u8]) {
    if bgra.as_chunks::<4>().0.iter().all(|px| px[3] == 0) {
        bgra.as_chunks_mut::<4>()
            .0
            .iter_mut()
            .for_each(|px| px[3] = 255);
        return;
    }
    for px in bgra.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thumbnail_without_alpha_becomes_opaque() {
        let mut px = vec![10, 20, 30, 0, 40, 50, 60, 0];
        straighten(&mut px);
        assert_eq!(px, [10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn premultiplied_icon_pixels_are_straightened() {
        // Half-transparent white, premultiplied; a clear pixel; an opaque one.
        let mut px = vec![128, 128, 128, 128, 0, 0, 0, 0, 1, 2, 3, 255];
        straighten(&mut px);
        assert_eq!(px, [255, 255, 255, 128, 0, 0, 0, 0, 1, 2, 3, 255]);
    }
}
