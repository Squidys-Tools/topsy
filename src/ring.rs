//! A battery- or load-style ring, drawn on a canvas: the stats widget's and a headset's.

use std::f32::consts::TAU;

use gpui::{Bounds, Pixels, Point, Window, fill, point, px, rgba, size};

/// A ring `diameter` across with a stroke `line` wide, filled clockwise from the top to
/// `fraction`, over a faint track of the whole circle.
pub fn paint(
    bounds: Bounds<Pixels>,
    (diameter, line): (f32, f32),
    fraction: f32,
    colour: u32,
    window: &mut Window,
) {
    let radius = (diameter - line) / 2.0;
    let centre = bounds.center();
    let at = |angle: f32| -> Point<Pixels> {
        point(
            centre.x + px(radius * angle.sin()),
            centre.y - px(radius * angle.cos()),
        )
    };
    let radii = point(px(radius), px(radius));
    let mut track = gpui::PathBuilder::stroke(px(line));
    track.move_to(at(0.0));
    track.arc_to(radii, px(0.0), false, true, at(TAU / 2.0));
    track.arc_to(radii, px(0.0), false, true, at(0.0));
    if let Ok(path) = track.build() {
        window.paint_path(path, rgba(colour << 8 | 0x33));
    }
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction <= 0.0 {
        return;
    }
    let solid = rgba(colour << 8 | 0xff);
    let end = fraction * TAU;
    let mut arc = gpui::PathBuilder::stroke(px(line));
    arc.move_to(at(0.0));
    if fraction >= 0.999 {
        arc.arc_to(radii, px(0.0), false, true, at(TAU / 2.0));
        arc.arc_to(radii, px(0.0), false, true, at(0.0));
    } else {
        arc.arc_to(radii, px(0.0), fraction > 0.5, true, at(end));
    }
    if let Ok(path) = arc.build() {
        window.paint_path(path, solid);
    }
    // PathBuilder's caps are square; dots at both ends round them off.
    for angle in [0.0, end] {
        let dot = at(angle);
        let half = line / 2.0;
        let cap = Bounds::new(
            point(dot.x - px(half), dot.y - px(half)),
            size(px(line), px(line)),
        );
        window.paint_quad(fill(cap, solid).corner_radii(px(half)));
    }
}
