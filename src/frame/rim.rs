//! Paints the glass's lit rim (`model::rim`) over the notch or basket.

use gpui::{
    Background, Pixels, Point, Window, hsla, linear_color_stop, linear_gradient, point, px,
};

use crate::model::rim::{Outline, strokes};

/// Paint the rim of `outline`, whose coordinates are relative to `origin`.
pub(super) fn paint(window: &mut Window, origin: Point<Pixels>, outline: Outline) {
    let at = |(x, y): (f32, f32)| point(origin.x + px(x), origin.y + px(y));
    let white = |alpha: f32| hsla(0.0, 0.0, 1.0, alpha);
    for stroke in strokes(outline) {
        for segment in stroke.segments {
            let (alpha_from, alpha_to) = (segment.alpha_from, segment.alpha_to);
            if alpha_from.max(alpha_to) < 0.002 {
                continue;
            }
            let mut path = gpui::PathBuilder::stroke(px(stroke.width));
            path.move_to(at(segment.from));
            path.line_to(at(segment.to));
            let Ok(path) = path.build() else {
                continue;
            };
            // Only the sides fade along their length, and they run up and down: a vertical
            // gradient, its top stop the end higher on screen.
            let fill: Background = if (alpha_from - alpha_to).abs() > 0.01 {
                let (top, bottom) = if segment.from.1 < segment.to.1 {
                    (alpha_from, alpha_to)
                } else {
                    (alpha_to, alpha_from)
                };
                linear_gradient(
                    180.0,
                    linear_color_stop(white(top), 0.0),
                    linear_color_stop(white(bottom), 1.0),
                )
            } else {
                white((alpha_from + alpha_to) / 2.0).into()
            };
            window.paint_path(path, fill);
        }
    }
}
