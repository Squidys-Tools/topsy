//! Where a PDF draws its pictures. A page's content moves and scales its drawing space with `cm`
//! between `q` and `Q`; the matrix in force at a picture's `Do` is what sets how big the picture
//! really is, whatever the page.

/// `[a b c d e f]` as PDF writes it: a point (x, y) goes to (ax + cy + e, bx + dy + f).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix(pub [f64; 6]);

impl Matrix {
    pub const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// This matrix's change of space first, then `next`'s.
    pub fn then(self, next: Matrix) -> Matrix {
        let (a, n) = (self.0, next.0);
        Matrix([
            a[0] * n[0] + a[1] * n[2],
            a[0] * n[1] + a[1] * n[3],
            a[2] * n[0] + a[3] * n[2],
            a[2] * n[1] + a[3] * n[3],
            a[4] * n[0] + a[5] * n[2] + n[4],
            a[4] * n[1] + a[5] * n[3] + n[5],
        ])
    }

    /// The width and height, in points, that the unit square (a picture) takes when drawn
    /// through this matrix, however it is turned. `None` if it is flat or not a number.
    pub fn shown(self) -> Option<(f64, f64)> {
        let m = self.0;
        let (w, h) = (m[0].hypot(m[1]), m[2].hypot(m[3]));
        (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some((w, h))
    }
}

/// The operations of a content stream that matter here.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Save,
    Restore,
    Concat(Matrix),
    /// `Do` of the XObject with this resource name.
    Draw(Vec<u8>),
}

/// The deepest `q` nesting honoured; a stream that opens more is damaged or hostile.
const MAX_DEPTH: usize = 256;

/// Each name `ops` draws, with the matrix then in force, starting from `start`.
pub fn draws(ops: &[Op], start: Matrix) -> Vec<(&[u8], Matrix)> {
    let (mut ctm, mut saved) = (start, Vec::new());
    let mut out = Vec::new();
    for op in ops {
        match op {
            Op::Save if saved.len() < MAX_DEPTH => saved.push(ctm),
            Op::Save => {}
            Op::Restore => ctm = saved.pop().unwrap_or(ctm),
            Op::Concat(m) => ctm = m.then(ctm),
            Op::Draw(name) => out.push((name.as_slice(), ctm)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scale(w: f64, h: f64) -> Matrix {
        Matrix([w, 0.0, 0.0, h, 0.0, 0.0])
    }

    fn draw(name: &str) -> Op {
        Op::Draw(name.as_bytes().to_vec())
    }

    #[test]
    fn a_picture_is_the_size_the_matrix_gives_it() {
        let ops = [Op::Save, Op::Concat(scale(300.0, 200.0)), draw("Im0")];
        let found = draws(&ops, Matrix::IDENTITY);
        assert_eq!(found[0].0, b"Im0");
        assert_eq!(found[0].1.shown(), Some((300.0, 200.0)));
    }

    #[test]
    fn a_turn_changes_the_axes_not_the_size() {
        // 90 degrees: the picture's width runs up the page.
        let turned = Matrix([0.0, 300.0, -200.0, 0.0, 400.0, 0.0]);
        assert_eq!(turned.shown(), Some((300.0, 200.0)));
        assert_eq!(Matrix([0.0; 6]).shown(), None);
        assert_eq!(Matrix([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0]).shown(), None);
    }

    #[test]
    fn matrices_compose_inner_first_and_q_undoes_them() {
        // A page scaled by 2, then a picture 100 x 50 inside it: 200 x 100 shown.
        let ops = [
            Op::Concat(scale(2.0, 2.0)),
            Op::Save,
            Op::Concat(scale(100.0, 50.0)),
            draw("A"),
            Op::Restore,
            draw("B"),
        ];
        let found = draws(&ops, Matrix::IDENTITY);
        assert_eq!(found[0].1.shown(), Some((200.0, 100.0)));
        assert_eq!(found[1].1.shown(), Some((2.0, 2.0)), "Q put the scale back");
        // The order matters when a move comes between: translate, then scale, is not the reverse.
        let moved = Matrix([1.0, 0.0, 0.0, 1.0, 10.0, 20.0]).then(scale(2.0, 3.0));
        assert_eq!(moved.0, [2.0, 0.0, 0.0, 3.0, 20.0, 60.0]);
    }

    #[test]
    fn a_form_starts_where_it_was_drawn() {
        let start = Matrix([0.5, 0.0, 0.0, 0.5, 0.0, 0.0]);
        let ops = [Op::Concat(scale(100.0, 100.0)), draw("Im")];
        let found = draws(&ops, start);
        assert_eq!(found[0].1.shown(), Some((50.0, 50.0)));
    }

    #[test]
    fn stray_and_runaway_q_are_survived() {
        let ops = [
            Op::Restore,
            Op::Concat(scale(4.0, 4.0)),
            Op::Restore,
            draw("A"),
        ];
        assert_eq!(draws(&ops, Matrix::IDENTITY)[0].1.shown(), Some((4.0, 4.0)));
        let deep: Vec<Op> = (0..MAX_DEPTH * 2).map(|_| Op::Save).collect();
        assert!(draws(&deep, Matrix::IDENTITY).is_empty());
    }
}
