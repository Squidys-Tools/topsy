//! A track's art, read from its media session's thumbnail: the notch's small square, the expanded
//! floating player's cover, and the colour the level bars take from it.

use std::sync::Arc;

use gpui::RenderImage;
use image::{Frame, RgbaImage};
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapBounds, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat,
    BitmapTransform, ColorManagementMode, ExifOrientationMode,
};
use windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties as Properties;

/// A track's art, the larger cover the expanded floating player shows, and the colour Now
/// Playing's bars take from it.
#[derive(Clone)]
pub struct Art {
    pub image: Arc<RenderImage>,
    pub cover: Arc<RenderImage>,
    pub accent: Option<[u8; 3]>,
}

/// The art's shorter side in physical pixels: in the notch's player, and as the cover.
#[derive(Clone, Copy)]
pub struct ArtSizes {
    pub art: u32,
    pub cover: u32,
}

/// The track's art at both sizes, decoded once.
pub(super) fn read(props: &Properties, sizes: ArtSizes) -> windows::core::Result<Art> {
    let stream = props.Thumbnail()?.OpenReadAsync()?.join()?;
    let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
    let (pixels, side) = square(&decoder, sizes.art)?;
    let accent = model::levels::accent(&pixels);
    let image = render_image(pixels, side)?;
    let cover = match square(&decoder, sizes.cover)? {
        // A small thumbnail is never scaled up, so both come out the same.
        (_, cover_side) if cover_side == side => image.clone(),
        (pixels, cover_side) => render_image(pixels, cover_side)?,
    };
    Ok(Art {
        image,
        cover,
        accent,
    })
}

/// The decoded art scaled down (never up) so its shorter side is `size`, and cropped to a
/// centred square: GPUI draws a covering image past its box (a video's 16:9 thumbnail spilled
/// out of the notch). BGRA pixels with straight alpha, and the square's side.
fn square(decoder: &BitmapDecoder, size: u32) -> windows::core::Result<(Vec<u8>, u32)> {
    let (w, h) = (decoder.PixelWidth()?, decoder.PixelHeight()?);
    let scale = (size as f32 / w.min(h).max(1) as f32).min(1.0);
    let (w, h) = (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    );
    let transform = BitmapTransform::new()?;
    transform.SetScaledWidth(w)?;
    transform.SetScaledHeight(h)?;
    transform.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
    let side = w.min(h);
    transform.SetBounds(BitmapBounds {
        X: (w - side) / 2,
        Y: (h - side) / 2,
        Width: side,
        Height: side,
    })?;
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Straight,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .join()?
        .DetachPixelData()?;
    Ok((pixels.to_vec(), side))
}

/// GPUI's images are BGRA with straight alpha, whatever the buffer's type says.
fn render_image(pixels: Vec<u8>, side: u32) -> windows::core::Result<Arc<RenderImage>> {
    let buffer = RgbaImage::from_raw(side, side, pixels).ok_or_else(windows::core::Error::empty)?;
    Ok(Arc::new(RenderImage::new(vec![Frame::new(buffer)])))
}
