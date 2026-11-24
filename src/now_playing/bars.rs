//! The level bars beside the title: the default output device's peak meter, read a frame at a
//! time while the bars are on screen and moving, and let go otherwise.

use std::time::{Duration, Instant};

use gpui::{Bounds, IntoElement, Styled, canvas, fill, point, px, rgb};

use crate::win32::meter::Meter;
use model::levels::{self, BARS, Levels};

/// Ink for a bar with no art to take a colour from.
const GLYPH: u32 = 0xebebeb;

/// How often a moving set of bars is redrawn.
pub(super) const FRAME: Duration = Duration::from_millis(levels::FRAME_MS);

#[derive(Default)]
pub(super) struct Bars {
    levels: Levels,
    /// Open only while the bars move, with when it was opened.
    meter: Option<(Meter, Instant)>,
}

impl Bars {
    /// The next frame: a new reading while `live`, else the meter let go and the bars settled.
    /// Whether the bars changed and need drawing.
    pub(super) fn frame(&mut self, live: bool) -> bool {
        if !live {
            self.meter = None;
            let moving = self.levels.bars() != [levels::REST; BARS];
            self.levels.rest();
            return moving;
        }
        // Reopened every second, so a change of output device (headphones in) is followed.
        let now = Instant::now();
        let stale = (self.meter.as_ref())
            .is_none_or(|(_, opened)| now.duration_since(*opened) >= Duration::from_secs(1));
        if stale {
            self.meter = Some((Meter::open(), now));
        }
        let peak = self.meter.as_ref().map_or(0.0, |(meter, _)| meter.peak());
        self.levels.push(peak);
        true
    }

    /// The bars, `height` tall, each `width` wide, in the art's `accent`.
    pub(super) fn draw(
        &self,
        height: f32,
        width: f32,
        accent: Option<[u8; 3]>,
    ) -> impl IntoElement + use<> {
        let heights = self.levels.bars();
        let color = match accent {
            Some([r, g, b]) => rgb(u32::from_be_bytes([0, r, g, b])),
            None => rgb(GLYPH),
        };
        let gap = width * 0.75;
        canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let origin = bounds.origin;
                for (i, h) in heights.iter().enumerate() {
                    let h = (height * h).max(width);
                    let x = i as f32 * (width + gap);
                    let bar = Bounds::new(
                        point(origin.x + px(x), origin.y + px((height - h) / 2.0)),
                        gpui::size(px(width), px(h)),
                    );
                    window.paint_quad(fill(bar, color).corner_radii(px(width / 2.0)));
                }
            },
        )
        .w(px(BARS as f32 * (width + gap) - gap))
        .h(px(height))
        .flex_shrink_0()
    }
}
