//! Landing a file under a size: the highest quality that fits, and a smaller picture when even
//! the lowest quality doesn't.

/// One try in a size search.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Attempt {
    pub quality: u8,
    /// Of the starting dimensions.
    pub scale: f64,
}

pub const SEARCH_MIN_QUALITY: u8 = 10;
pub const SEARCH_MAX_QUALITY: u8 = 95;

/// Finds the best encoding at or under `target` bytes: the highest quality that fits at full
/// size, and if even the lowest doesn't, a smaller size. `encode` makes one try and gives back
/// its result and size. Returns the best that fits, or the smallest try when nothing does (the
/// caller says so).
pub fn fit_to_size<T, E>(
    target: u64,
    mut encode: impl FnMut(Attempt) -> Result<(T, u64), E>,
) -> Result<(T, u64, bool), E> {
    let mut best: Option<(T, u64)> = None;
    let mut smallest: Option<(T, u64)> = None;
    let keep = |r: (T, u64), best: &mut Option<(T, u64)>, smallest: &mut Option<(T, u64)>| {
        let size = r.1;
        if size <= target {
            if best.as_ref().is_none_or(|b| size > b.1) {
                *best = Some(r);
            }
        } else if smallest.as_ref().is_none_or(|s| size < s.1) {
            *smallest = Some(r);
        }
        size
    };
    let mut scale = 1.0f64;
    for _ in 0..6 {
        let top = keep(
            encode(Attempt {
                quality: SEARCH_MAX_QUALITY,
                scale,
            })?,
            &mut best,
            &mut smallest,
        );
        if top <= target {
            break;
        }
        let bottom = keep(
            encode(Attempt {
                quality: SEARCH_MIN_QUALITY,
                scale,
            })?,
            &mut best,
            &mut smallest,
        );
        if bottom > target {
            // Size goes roughly with area, so shrink each side by the square root.
            let ratio = (target as f64 / bottom as f64).sqrt() * 0.95;
            scale *= ratio.clamp(0.1, 0.9);
            continue;
        }
        let (mut lo, mut hi) = ((SEARCH_MIN_QUALITY, bottom), (SEARCH_MAX_QUALITY, top));
        for _ in 0..5 {
            if hi.0 - lo.0 <= 1 || lo.1 as f64 >= target as f64 * 0.97 {
                break;
            }
            // Size grows about exponentially with quality: interpolate on its logarithm.
            let (l, h, t) = ((lo.1 as f64).ln(), (hi.1 as f64).ln(), (target as f64).ln());
            let guess = f64::from(lo.0) + (t - l) / (h - l) * f64::from(hi.0 - lo.0);
            let q = (guess.floor() as u8).clamp(lo.0 + 1, hi.0 - 1);
            let size = keep(
                encode(Attempt { quality: q, scale })?,
                &mut best,
                &mut smallest,
            );
            if size <= target {
                lo = (q, size)
            } else {
                hi = (q, size)
            }
        }
        break;
    }
    Ok(match (best, smallest) {
        (Some((t, s)), _) => (t, s, true),
        (None, Some((t, s))) => (t, s, false),
        (None, None) => unreachable!("every round makes at least one try"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pretend encoder: size grows exponentially with quality and with area.
    fn model(base: f64) -> impl FnMut(Attempt) -> Result<(Attempt, u64), ()> {
        move |a| {
            Ok((
                a,
                (base * (f64::from(a.quality) / 25.0).exp() * a.scale * a.scale) as u64,
            ))
        }
    }

    #[test]
    fn size_search_lands_just_under_the_target() {
        let mut tries = 0;
        let mut enc = model(100_000.0);
        let (a, size, fits) = fit_to_size(1_000_000, |a| {
            tries += 1;
            enc(a)
        })
        .unwrap();
        assert!(fits && size <= 1_000_000 && size > 900_000, "{size}");
        assert_eq!(a.scale, 1.0);
        assert!(tries <= 7, "{tries} encodes");
    }

    #[test]
    fn size_search_shrinks_when_quality_is_not_enough() {
        let (a, size, fits) = fit_to_size(50_000, model(400_000.0)).unwrap();
        assert!(fits && size <= 50_000, "{size}");
        assert!(a.scale < 1.0);
    }

    #[test]
    fn size_search_takes_top_quality_when_it_already_fits() {
        let (a, _, fits) = fit_to_size(u64::MAX, model(1.0)).unwrap();
        assert!(fits);
        assert_eq!(a.quality, SEARCH_MAX_QUALITY);
    }

    #[test]
    fn an_impossible_target_returns_the_smallest_try() {
        let (_, size, fits) = fit_to_size(1, |a: Attempt| {
            Ok::<_, ()>(((), 1000 + u64::from(a.quality)))
        })
        .unwrap();
        assert!(!fits);
        assert_eq!(size, 1000 + u64::from(SEARCH_MIN_QUALITY));
    }
}
