//! The glass's refraction: near the notch's edge each pixel shows what lies a little further out,
//! as through the rim of a convex lens (Notchy's take on Droppy's liquid glass). The bend grows
//! towards the edge and dies away over its last pixels, so the rim shows no hard line.

/// A rounded rectangle, in the pixels of the map it is painted into.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rounded {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
}

impl Rounded {
    /// Signed distance from `p` to the edge (negative inside), and the outward normal there.
    fn distance(&self, p: (f32, f32)) -> (f32, (f32, f32)) {
        let half = (self.width / 2.0, self.height / 2.0);
        let radius = self.radius.min(half.0).min(half.1).max(0.0);
        let centred = (p.0 - self.x - half.0, p.1 - self.y - half.1);
        let q = (
            centred.0.abs() - (half.0 - radius),
            centred.1.abs() - (half.1 - radius),
        );
        let outside = (q.0.max(0.0), q.1.max(0.0));
        let corner = outside.0.hypot(outside.1);
        let d = corner + q.0.max(q.1).min(0.0) - radius;
        let normal = if corner > 0.0 {
            (outside.0 / corner, outside.1 / corner)
        } else if q.0 > q.1 {
            (1.0, 0.0)
        } else {
            (0.0, 1.0)
        };
        (
            d,
            (normal.0 * centred.0.signum(), normal.1 * centred.1.signum()),
        )
    }
}

/// How the rim bends, in pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Lens {
    /// How far in from the edge the bend reaches.
    pub band: f32,
    /// The furthest any pixel looks outward.
    pub strength: f32,
    /// Over how many pixels at the very edge the bend dies away.
    pub fade: f32,
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Lens {
    /// How far the pixel at `p` looks away from itself: outward along the edge's normal.
    pub fn bend(&self, shape: &Rounded, p: (f32, f32)) -> (f32, f32) {
        let (d, normal) = shape.distance(p);
        if d >= 0.0 || d <= -self.band {
            return (0.0, 0.0);
        }
        let t = 1.0 + d / self.band;
        let amount = self.strength * t * t * t * (1.0 - smoothstep(-self.fade, 0.0, d));
        (normal.0 * amount, normal.1 * amount)
    }

    /// How much of the bent glass shows at `p`: all of it over the band, none further in, where
    /// nothing bends and the plain glass can show instead.
    pub fn reach(&self, shape: &Rounded, p: (f32, f32)) -> f32 {
        let (d, _) = shape.distance(p);
        smoothstep(-self.band, -self.band * 0.5, d)
    }

    /// Paint the bend for `shape` into a `width` x `height` BGRA map, as a displacement map
    /// reads it: red holds the x bend and green the y bend, each `0.5 + bend / (2 * strength)`,
    /// and blue its [`reach`](Self::reach). Pixels the bend doesn't reach hold 0.5, which moves
    /// nothing.
    pub fn paint(&self, shape: &Rounded, width: usize, height: usize, map: &mut Vec<u8>) {
        map.clear();
        map.resize(width * height * 4, 0);
        let span = 2.0 * self.strength.max(f32::EPSILON);
        let encode = |bend: f32| ((0.5 + bend / span) * 255.0).round().clamp(0.0, 255.0) as u8;
        for y in 0..height {
            for x in 0..width {
                let p = (x as f32 + 0.5, y as f32 + 0.5);
                let (dx, dy) = self.bend(shape, p);
                let i = (y * width + x) * 4;
                map[i] = (self.reach(shape, p) * 255.0).round() as u8;
                map[i + 1] = encode(dy);
                map[i + 2] = encode(dx);
                map[i + 3] = 255;
            }
        }
    }
}

/// The tint over the bent glass: solid black down to `black_until`, fading to `bottom` (an alpha,
/// 0 to 1) at the shape's foot, and clearer over the rim below the black, so what the rim bends
/// in shows bright, as at the edge of a real lens. In pixels, like the shape.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shade {
    pub black_until: f32,
    pub bottom: f32,
    /// How far in from the edge the rim clears.
    pub band: f32,
    /// How much of the tint is lifted at the very edge.
    pub clear: f32,
}

impl Shade {
    /// How opaque the black is at `p`.
    pub fn alpha(&self, shape: &Rounded, p: (f32, f32)) -> f32 {
        let foot = shape.y + shape.height;
        let down = ((p.1 - self.black_until) / (foot - self.black_until).max(f32::EPSILON))
            .clamp(0.0, 1.0);
        let tint = 1.0 + (self.bottom - 1.0) * down;
        let (d, _) = shape.distance(p);
        let rim = if d < 0.0 && d > -self.band {
            let t = 1.0 + d / self.band;
            t * t
        } else {
            0.0
        };
        let below = smoothstep(self.black_until * 0.5, self.black_until, p.1);
        tint * (1.0 - self.clear * rim * below)
    }

    /// Paint the tint for `shape` into a `width` x `height` premultiplied BGRA map: black, so
    /// only the alpha is set.
    pub fn paint(&self, shape: &Rounded, width: usize, height: usize, map: &mut Vec<u8>) {
        map.clear();
        map.resize(width * height * 4, 0);
        for y in 0..height {
            for x in 0..width {
                let alpha = self.alpha(shape, (x as f32 + 0.5, y as f32 + 0.5));
                map[(y * width + x) * 4 + 3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LENS: Lens = Lens {
        band: 28.0,
        strength: 22.0,
        fade: 4.0,
    };
    const SHAPE: Rounded = Rounded {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 120.0,
        radius: 20.0,
    };

    #[test]
    fn the_middle_and_the_outside_are_left_alone() {
        assert_eq!(LENS.bend(&SHAPE, (200.0, 60.0)), (0.0, 0.0));
        assert_eq!(LENS.bend(&SHAPE, (200.0, 125.0)), (0.0, 0.0));
        assert_eq!(LENS.bend(&SHAPE, (200.0, 120.0)), (0.0, 0.0));
    }

    #[test]
    fn near_an_edge_it_looks_outward_across_it() {
        let (x, y) = LENS.bend(&SHAPE, (200.0, 110.0));
        assert!(x.abs() < 1e-4 && y > 5.0, "bottom: {x}, {y}");
        let (x, y) = LENS.bend(&SHAPE, (390.0, 60.0));
        assert!(x > 5.0 && y.abs() < 1e-4, "right: {x}, {y}");
        let (x, y) = LENS.bend(&SHAPE, (10.0, 60.0));
        assert!(x < -5.0 && y.abs() < 1e-4, "left: {x}, {y}");
        let (x, y) = LENS.bend(&SHAPE, (392.0, 112.0));
        assert!(x > 1.0 && (x - y).abs() < 1e-3, "corner: {x}, {y}");
    }

    #[test]
    fn the_bend_grows_towards_the_edge_then_dies_away_on_it() {
        let at = |inside: f32| LENS.bend(&SHAPE, (200.0, 120.0 - inside)).1;
        assert!(at(27.0) < at(14.0) && at(14.0) < at(6.0));
        assert!(at(6.0) <= LENS.strength);
        assert!(at(0.5) < at(6.0) / 2.0);
    }

    #[test]
    fn the_map_holds_the_bend_around_a_half() {
        let shape = Rounded {
            x: 0.0,
            y: 0.0,
            width: 60.0,
            height: 60.0,
            radius: 10.0,
        };
        let mut map = Vec::new();
        LENS.paint(&shape, 60, 60, &mut map);
        assert_eq!(map.len(), 60 * 60 * 4);
        let pixel = |x: usize, y: usize| &map[(y * 60 + x) * 4..][..4];
        // The centre is far enough in to stay put.
        assert_eq!(pixel(30, 30), [0, 128, 128, 255]);
        let right = pixel(55, 30);
        assert!(
            right[2] > 128 && right[1] == 128 && right[0] == 255,
            "{right:?}"
        );
        let top = pixel(30, 4);
        assert!(top[1] < 128 && top[2] == 128, "{top:?}");
    }

    #[test]
    fn the_shade_is_black_on_top_and_clears_at_the_rim_below() {
        let shade = Shade {
            black_until: 45.0,
            bottom: 0.44,
            band: 28.0,
            clear: 0.8,
        };
        let middle = |y: f32| shade.alpha(&SHAPE, (200.0, y));
        assert_eq!(middle(10.0), 1.0);
        assert!(middle(45.0) > 0.99);
        // Midway down, well inside, the plain tint.
        assert!((middle(82.0) - 0.72).abs() < 0.01, "{}", middle(82.0));
        // The rim clears towards the edge, below the black only.
        assert!(middle(118.0) < middle(100.0) / 2.0);
        assert_eq!(shade.alpha(&SHAPE, (2.0, 10.0)), 1.0);
        assert!(shade.alpha(&SHAPE, (2.0, 90.0)) < 0.3);
    }
}
