//! The shell's own picture of a file, as Explorer shows it: a thumbnail where the file has one
//! (photos, videos, documents with previews), its icon otherwise.
//!
//! Asking can read the file and wake a thumbnail handler, which takes a while for a video or a
//! cold network path, so call it from a worker thread, never the UI thread.

use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;

use gpui::{App, RenderImage};
use image::{Frame, RgbaImage};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HBITMAP,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
};
use windows::core::HSTRING;

/// The picture for `path`, about `size` physical pixels square. Call inside
/// [`crate::win32::with_com`].
pub fn load(path: &Path, size: i32) -> Option<Arc<RenderImage>> {
    let (width, height, mut pixels) = unsafe { shell_bitmap(path, size) }?;
    straighten(&mut pixels);
    // GPUI's images are BGRA with straight alpha, whatever the buffer's type says.
    let buffer = RgbaImage::from_raw(width, height, pixels)?;
    Some(Arc::new(RenderImage::new(vec![Frame::new(buffer)])))
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
    let _ = unsafe { DeleteObject(bitmap.into()) };
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
    let _ = unsafe { DeleteDC(dc) };
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
