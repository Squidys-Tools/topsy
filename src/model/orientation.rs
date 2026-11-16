//! The eight ways to turn a picture on a grid (the dihedral group D4), numbered as EXIF numbers
//! its orientation tag. A photo's tag and the user's rotation compose into one of these, so a
//! file is turned once, never twice.

/// What has to happen to the stored pixels to show them upright, in EXIF's numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Normal = 1,
    FlipHorizontal = 2,
    Rotate180 = 3,
    FlipVertical = 4,
    Transpose = 5,
    Rotate90 = 6,
    Transverse = 7,
    Rotate270 = 8,
}

use Orientation::*;

const ALL: [Orientation; 8] = [
    Normal,
    FlipHorizontal,
    Rotate180,
    FlipVertical,
    Transpose,
    Rotate90,
    Transverse,
    Rotate270,
];

impl Orientation {
    pub fn from_exif(value: u16) -> Self {
        ALL.get(usize::from(value).wrapping_sub(1))
            .copied()
            .unwrap_or(Normal)
    }

    /// As steps: transpose first (if set), then mirror left-right, then top-bottom. Rotate90 is
    /// clockwise.
    pub fn parts(self) -> (bool, bool, bool) {
        match self {
            Normal => (false, false, false),
            FlipHorizontal => (false, true, false),
            Rotate180 => (false, true, true),
            FlipVertical => (false, false, true),
            Transpose => (true, false, false),
            Rotate90 => (true, true, false),
            Transverse => (true, true, true),
            Rotate270 => (true, false, true),
        }
    }

    /// The 2x2 matrix acting on centred (x, y) coordinates.
    fn matrix(self) -> [[i8; 2]; 2] {
        let (t, fh, fv) = self.parts();
        let mut m = if t {
            [[0, 1], [1, 0]]
        } else {
            [[1, 0], [0, 1]]
        };
        if fh {
            m[0] = [-m[0][0], -m[0][1]];
        }
        if fv {
            m[1] = [-m[1][0], -m[1][1]];
        }
        m
    }

    /// `self` first, then `next`.
    pub fn then(self, next: Orientation) -> Orientation {
        let (a, b) = (self.matrix(), next.matrix());
        let m: [[i8; 2]; 2] =
            std::array::from_fn(|i| std::array::from_fn(|j| b[i][0] * a[0][j] + b[i][1] * a[1][j]));
        ALL.into_iter().find(|o| o.matrix() == m).unwrap_or(Normal)
    }

    pub fn swaps_axes(self) -> bool {
        self.parts().0
    }

    /// Where pixel (x, y) of a `w` x `h` source lands.
    pub fn map(self, x: u32, y: u32, w: u32, h: u32) -> (u32, u32) {
        let (t, fh, fv) = self.parts();
        let (mut nx, mut ny, nw, nh) = if t { (y, x, h, w) } else { (x, y, w, h) };
        if fh {
            nx = nw - 1 - nx;
        }
        if fv {
            ny = nh - 1 - ny;
        }
        (nx, ny)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations_compose_like_turns() {
        assert_eq!(Rotate90.then(Rotate90), Rotate180);
        assert_eq!(Rotate90.then(Rotate270), Normal);
        assert_eq!(Rotate180.then(Rotate90), Rotate270);
        assert_eq!(FlipHorizontal.then(FlipHorizontal), Normal);
        assert_eq!(FlipHorizontal.then(FlipVertical), Rotate180);
        for o in ALL {
            assert_eq!(Normal.then(o), o);
            assert_eq!(o.then(Normal), o);
        }
    }

    #[test]
    fn map_agrees_with_composition() {
        let (w, h) = (5, 3);
        for a in ALL {
            for b in ALL {
                let ab = a.then(b);
                for y in 0..h {
                    for x in 0..w {
                        let (x1, y1) = a.map(x, y, w, h);
                        let (w1, h1) = if a.swaps_axes() { (h, w) } else { (w, h) };
                        assert_eq!(
                            b.map(x1, y1, w1, h1),
                            ab.map(x, y, w, h),
                            "{a:?} then {b:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn rotate90_is_clockwise() {
        // The bottom-left pixel of a 4x2 picture ends up top-left.
        assert_eq!(Rotate90.map(0, 1, 4, 2), (0, 0));
        assert_eq!(Rotate270.map(0, 0, 4, 2), (0, 3));
        assert_eq!(Orientation::from_exif(6), Rotate90);
        assert_eq!(Orientation::from_exif(0), Normal);
        assert_eq!(Orientation::from_exif(9), Normal);
    }
}
