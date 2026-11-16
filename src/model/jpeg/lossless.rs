//! Rotations and flips done on the DCT blocks, as jpegtran does: the picture turns without a
//! decode, so nothing is lost. A flip moves the blocks and negates the odd frequencies along the
//! flipped axis; a transpose swaps each block's rows and columns, its quantisation table and the
//! sampling factors.

use crate::model::orientation::Orientation;

use super::coeffs::{Block, Jpeg};

/// Whether `o` can turn this image without touching a pixel: every edge it mirrors must fall on
/// a whole MCU, or the partial blocks would land on the wrong side.
pub fn is_exact(jpeg: &Jpeg, o: Orientation) -> bool {
    let (t, fh, fv) = o.parts();
    let (mcu_w, mcu_h) = (8 * jpeg.hmax(), 8 * jpeg.vmax());
    let w_ok = jpeg.width.is_multiple_of(mcu_w);
    let h_ok = jpeg.height.is_multiple_of(mcu_h);
    // After a transpose, a horizontal flip mirrors what was the height, and vice versa.
    let (mirror_w, mirror_h) = if t { (fv, fh) } else { (fh, fv) };
    (!mirror_w || w_ok) && (!mirror_h || h_ok)
}

/// Applies `o` to the coefficients. Callers check [`is_exact`] first.
pub fn transform(jpeg: &Jpeg, o: Orientation) -> Jpeg {
    let (t, fh, fv) = o.parts();
    let (width, height) = if t {
        (jpeg.height, jpeg.width)
    } else {
        (jpeg.width, jpeg.height)
    };
    let mut out = Jpeg {
        width,
        height,
        components: jpeg.components.clone(),
        qtables: jpeg.qtables,
        extras: jpeg.extras.clone(),
    };
    if t {
        for q in &mut out.qtables {
            *q = transpose(q);
        }
        for c in &mut out.components {
            std::mem::swap(&mut c.h, &mut c.v);
        }
    }
    out.allocate();
    for (src, dst) in jpeg.components.iter().zip(&mut out.components) {
        let (uw, uh) = jpeg.used_blocks(src);
        let (dw, dh) = if t { (uh, uw) } else { (uw, uh) };
        for y in 0..uh {
            for x in 0..uw {
                let (mut nx, mut ny) = if t { (y, x) } else { (x, y) };
                if fh {
                    nx = dw - 1 - nx;
                }
                if fv {
                    ny = dh - 1 - ny;
                }
                let mut b = src.blocks[y * src.bw + x];
                if t {
                    b = transpose(&b);
                }
                if fh {
                    negate(&mut b, |_, col| col % 2 == 1);
                }
                if fv {
                    negate(&mut b, |row, _| row % 2 == 1);
                }
                dst.blocks[ny * dst.bw + nx] = b;
            }
        }
    }
    out
}

fn transpose<T: Copy>(b: &[T; 64]) -> [T; 64] {
    std::array::from_fn(|i| b[(i % 8) * 8 + i / 8])
}

fn negate(b: &mut Block, odd: impl Fn(usize, usize) -> bool) {
    for (i, v) in b.iter_mut().enumerate() {
        if odd(i / 8, i % 8) {
            *v = -*v;
        }
    }
}
