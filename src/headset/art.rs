//! The generic headsets' turntables, compiled in (`devices/generic/<shape>/spin.png`, rendered
//! by `cargo devices generic`) and cut into frames the first time a shape shows.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::RenderImage;
use image::{Frame, ImageFormat, imageops};

use model::headset::Form;
use model::headset::spin::{FRAME, FRAMES, cell};

pub type Frames = Rc<[Arc<RenderImage>]>;

#[derive(Default)]
pub struct Art(RefCell<HashMap<Form, Frames>>);

impl Art {
    pub fn frames(&self, form: Form) -> Option<Frames> {
        if let Some(frames) = self.0.borrow().get(&form) {
            return Some(Rc::clone(frames));
        }
        let frames = cut(sheet(form))?;
        self.0.borrow_mut().insert(form, Rc::clone(&frames));
        Some(frames)
    }
}

fn sheet(form: Form) -> &'static [u8] {
    match form {
        Form::StemBuds => include_bytes!("../../devices/generic/stem-buds/spin.png"),
        Form::Buds => include_bytes!("../../devices/generic/buds/spin.png"),
        Form::OverEar => include_bytes!("../../devices/generic/over-ear/spin.png"),
        Form::Speaker => include_bytes!("../../devices/generic/speaker/spin.png"),
    }
}

fn cut(png: &[u8]) -> Option<Frames> {
    let mut sheet = image::load_from_memory_with_format(png, ImageFormat::Png)
        .ok()?
        .into_rgba8();
    // GPUI's images are BGRA.
    for pixel in sheet.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    let frames = (0..FRAMES).map(|index| {
        let (x, y) = cell(index);
        let frame = imageops::crop_imm(&sheet, x, y, FRAME, FRAME).to_image();
        Arc::new(RenderImage::new(vec![Frame::new(frame)]))
    });
    Some(frames.collect())
}
