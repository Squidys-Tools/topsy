//! The glass's lit rim: a thin highlight along the edge and a soft glow just inside it, both
//! brighter where the edge faces toward (or straight away from) a light at the top left, as on
//! Liquid Glass. The rim fades out up the sides, where the notch turns black.

/// Toward the light, in screen coordinates (y down): up and a little left.
const LIGHT: (f32, f32) = (-0.45, -1.0);
/// The edge highlight's opacity where it catches the light fully.
const EDGE: f32 = 0.5;
/// How much of the edge highlight is there even facing across the light.
const EDGE_FLOOR: f32 = 0.3;
/// The inner glow: its opacity at the edge facing across the light, and what facing it adds.
const GLOW: f32 = 0.015;
const GLOW_LIT: f32 = 0.06;
/// How far the glow reaches in, and the width of each band it is drawn in.
const GLOW_DEPTH: f32 = 16.0;
const GLOW_BAND: f32 = 3.0;
/// Segments per rounded corner.
const CORNER_STEPS: usize = 6;

/// The rim's path: the centre line of a 1 px stroke along the sides and bottom, rounded at the
/// bottom corners. `top` is where it starts, fully faded, on both sides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outline {
    pub left: f32,
    pub right: f32,
    pub bottom: f32,
    pub top: f32,
    pub radius: f32,
}

/// A straight piece of a stroke, with its opacity at each end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub alpha_from: f32,
    pub alpha_to: f32,
}

/// One stroke of the rim: its width and its pieces, end to end.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub width: f32,
    pub segments: Vec<Segment>,
}

/// The edge highlight, then the glow's bands from the outside in.
pub fn strokes(outline: Outline) -> Vec<Stroke> {
    let length = (LIGHT.0 * LIGHT.0 + LIGHT.1 * LIGHT.1).sqrt();
    let light = (LIGHT.0 / length, LIGHT.1 / length);
    // Lit by how squarely the edge faces the light, from either side.
    let facing = |normal: (f32, f32)| (normal.0 * light.0 + normal.1 * light.1).abs();
    let fade =
        |y: f32| ((y - outline.top) / (outline.bottom - outline.top).max(1.0)).clamp(0.0, 1.0);

    let edge = Stroke {
        width: 1.0,
        segments: path(outline, 0.0, |point, normal| {
            let lit = facing(normal);
            EDGE * (EDGE_FLOOR + (1.0 - EDGE_FLOOR) * lit * lit) * fade(point.1)
        }),
    };
    let mut strokes = vec![edge];
    let mut inset = 0.5 + GLOW_BAND / 2.0;
    while inset < GLOW_DEPTH {
        let falloff = (1.0 - inset / GLOW_DEPTH).powi(2);
        strokes.push(Stroke {
            width: GLOW_BAND,
            segments: path(outline, inset, |point, normal| {
                (GLOW + GLOW_LIT * facing(normal)) * falloff * fade(point.1)
            }),
        });
        inset += GLOW_BAND;
    }
    strokes
}

/// The outline moved `inset` inwards, in pieces whose ends are weighed by `alpha` from their
/// point and outward normal.
fn path(
    outline: Outline,
    inset: f32,
    alpha: impl Fn((f32, f32), (f32, f32)) -> f32,
) -> Vec<Segment> {
    let (left, right) = (outline.left + inset, outline.right - inset);
    let (top, bottom) = (outline.top, outline.bottom - inset);
    if right <= left || bottom <= top {
        return Vec::new();
    }
    let radius = (outline.radius - inset)
        .max(0.0)
        .min(bottom - top)
        .min((right - left) / 2.0);

    let mut segments = Vec::new();
    let mut push = |from: (f32, f32), to: (f32, f32), from_normal, to_normal| {
        if (to.0 - from.0).hypot(to.1 - from.1) > 0.01 {
            segments.push(Segment {
                from,
                to,
                alpha_from: alpha(from, from_normal),
                alpha_to: alpha(to, to_normal),
            });
        }
    };
    let (out_left, down, out_right) = ((-1.0, 0.0), (0.0, 1.0), (1.0, 0.0));
    // A quarter turn about `center`, its normal swinging from angle `from` to `from - 90°`.
    let corner = |center: (f32, f32), from: f32, push: &mut dyn FnMut(_, _, _, _)| {
        let at = |step: usize| {
            let angle = from - std::f32::consts::FRAC_PI_2 * step as f32 / CORNER_STEPS as f32;
            let normal = (angle.cos(), angle.sin());
            (
                (center.0 + radius * normal.0, center.1 + radius * normal.1),
                normal,
            )
        };
        for step in 0..CORNER_STEPS {
            let ((from, from_normal), (to, to_normal)) = (at(step), at(step + 1));
            push(from, to, from_normal, to_normal);
        }
    };
    push((left, top), (left, bottom - radius), out_left, out_left);
    corner(
        (left + radius, bottom - radius),
        std::f32::consts::PI,
        &mut push,
    );
    push(
        (left + radius, bottom),
        (right - radius, bottom),
        down,
        down,
    );
    corner(
        (right - radius, bottom - radius),
        std::f32::consts::FRAC_PI_2,
        &mut push,
    );
    push((right, bottom - radius), (right, top), out_right, out_right);
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRAY: Outline = Outline {
        left: 0.5,
        right: 399.5,
        bottom: 117.5,
        top: 73.0,
        radius: 20.0,
    };

    fn alpha_near(stroke: &Stroke, at: (f32, f32)) -> f32 {
        let distance = |p: (f32, f32)| (p.0 - at.0).hypot(p.1 - at.1);
        stroke
            .segments
            .iter()
            .flat_map(|s| [(s.from, s.alpha_from), (s.to, s.alpha_to)])
            .min_by(|a, b| distance(a.0).total_cmp(&distance(b.0)))
            .map_or(0.0, |(_, alpha)| alpha)
    }

    #[test]
    fn the_light_catches_the_bottom_right_corner_more_than_the_bottom_left() {
        let edge = &strokes(TRAY)[0];
        let corner = |x: f32| {
            (
                x,
                TRAY.bottom - TRAY.radius * (1.0 - std::f32::consts::FRAC_1_SQRT_2),
            )
        };
        let bottom_left = alpha_near(edge, corner(TRAY.left + TRAY.radius * 0.29));
        let bottom_right = alpha_near(edge, corner(TRAY.right - TRAY.radius * 0.29));
        assert!(
            bottom_right > 2.0 * bottom_left,
            "{bottom_left} {bottom_right}"
        );
        assert!(bottom_right <= EDGE);
    }

    #[test]
    fn the_rim_fades_out_up_the_sides() {
        for stroke in strokes(TRAY) {
            let first = stroke.segments.first().map(|s| s.alpha_from);
            let last = stroke.segments.last().map(|s| s.alpha_to);
            assert_eq!(first, Some(0.0));
            assert_eq!(last, Some(0.0));
        }
    }

    #[test]
    fn the_glow_dims_inward_and_its_strokes_are_unbroken() {
        let all = strokes(TRAY);
        assert!(all.len() > 3);
        let middle = |s: &Stroke| alpha_near(s, ((TRAY.left + TRAY.right) / 2.0, TRAY.bottom));
        for pair in all[1..].windows(2) {
            assert!(middle(&pair[0]) > middle(&pair[1]));
        }
        for stroke in &all {
            for pair in stroke.segments.windows(2) {
                assert_eq!(pair[0].to, pair[1].from);
            }
        }
    }

    #[test]
    fn a_closed_notch_has_no_rim() {
        let flat = Outline {
            bottom: TRAY.top,
            ..TRAY
        };
        assert!(strokes(flat).iter().all(|s| s.segments.is_empty()));
    }
}
