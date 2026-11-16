//! Pixel work shared by the image Transforms: Lanczos scaling that doesn't darken soft edges,
//! cropping, and flattening transparency for formats without it.

use image::imageops::{self, FilterType};
use image::{Rgba32FImage, RgbaImage};

/// `image` scaled to `w` x `h` with Lanczos3. Transparent pictures are scaled premultiplied, so
/// the colour under a transparent pixel can't bleed into its neighbours.
pub fn scale(image: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    if image.dimensions() == (w, h) {
        return image.clone();
    }
    if image.pixels().all(|p| p.0[3] == 255) {
        return imageops::resize(image, w, h, FilterType::Lanczos3);
    }
    let (sw, sh) = image.dimensions();
    let pre: Vec<f32> = image
        .pixels()
        .flat_map(|p| {
            let a = f32::from(p.0[3]) / 255.0;
            [
                f32::from(p.0[0]) * a,
                f32::from(p.0[1]) * a,
                f32::from(p.0[2]) * a,
                a * 255.0,
            ]
        })
        .collect();
    let pre = Rgba32FImage::from_raw(sw, sh, pre).expect("same pixel count");
    let out = imageops::resize(&pre, w, h, FilterType::Lanczos3);
    let data = out
        .pixels()
        .flat_map(|p| {
            let a = p.0[3].clamp(0.0, 255.0);
            let un = |c: f32| {
                if a < 0.5 {
                    0
                } else {
                    (c * 255.0 / a).round().clamp(0.0, 255.0) as u8
                }
            };
            [un(p.0[0]), un(p.0[1]), un(p.0[2]), a.round() as u8]
        })
        .collect();
    RgbaImage::from_raw(w, h, data).expect("same pixel count")
}

pub fn crop(image: &RgbaImage, (x, y, w, h): (u32, u32, u32, u32)) -> RgbaImage {
    imageops::crop_imm(image, x, y, w, h).to_image()
}

/// RGB with transparency composited onto white, as a viewer shows it.
pub fn flatten(image: &RgbaImage) -> Vec<u8> {
    image
        .pixels()
        .flat_map(|p| {
            let a = u32::from(p.0[3]);
            let over = |c: u8| ((u32::from(c) * a + 255 * (255 - a) + 127) / 255) as u8;
            [over(p.0[0]), over(p.0[1]), over(p.0[2])]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_a_soft_edge_does_not_pull_in_the_hidden_colour() {
        // Opaque white beside fully transparent black: a straight-alpha filter greys the edge.
        let img = RgbaImage::from_fn(64, 8, |x, _| {
            if x < 32 {
                image::Rgba([255; 4])
            } else {
                image::Rgba([0, 0, 0, 0])
            }
        });
        let out = scale(&img, 16, 2);
        for p in out.pixels().filter(|p| p.0[3] > 8) {
            assert!(p.0[0] >= 250, "{p:?}");
        }
    }

    #[test]
    fn flattening_lands_on_white() {
        let img = RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0]));
        assert_eq!(flatten(&img), vec![255, 255, 255]);
        let half = RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 128]));
        assert_eq!(flatten(&half), vec![127, 127, 127]);
    }
}
