//! What the notch's player and the floating player are both made of: the art, the progress line
//! and the media buttons.

use std::sync::Arc;

use gpui::{
    ClickEvent, Div, FontWeight, ObjectFit, RenderImage, Stateful, div, img, prelude::*, px,
    relative, rgb, rgba,
};

use crate::icons::{self, Icon};
use crate::log;
use crate::now_playing::session::{self as media, Button};
use model::media::{Progress, clock};

pub(super) const DIM: u32 = 0xffffff80;
pub(super) const GLYPH: u32 = 0xebebeb;
const TIMES: f32 = 12.0;
const TIME_WIDTH: f32 = 48.0;
const BAR: f32 = 8.0;

/// Elapsed, the bar, and what is left, `TIMES` tall. Without a length (a live stream) the bar
/// stays empty and the times blank, so the buttons don't jump.
pub(super) fn timeline(progress: Option<Progress>) -> Div {
    let time = |text: String| {
        div()
            .w(px(TIME_WIDTH))
            .flex_shrink_0()
            .text_size(px(12.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgba(DIM))
            .child(text)
    };
    let (elapsed, left, fraction) = match progress {
        Some(p) => (
            clock(p.elapsed),
            format!("−{}", clock(p.remaining())),
            p.fraction(),
        ),
        None => (String::new(), String::new(), 0.0),
    };
    div()
        .h(px(TIMES))
        .flex()
        .flex_row()
        .items_center()
        .child(time(elapsed))
        .child(
            div()
                .flex_1()
                .h(px(BAR))
                .rounded_full()
                .bg(rgba(0xffffff26))
                .child(
                    div()
                        .h_full()
                        .w(relative(fraction))
                        .rounded_full()
                        .bg(rgba(0xffffff99)),
                ),
        )
        .child(time(left).flex().justify_end())
}

pub(super) fn art(
    art: Option<&Arc<RenderImage>>,
    side: f32,
    radius: f32,
) -> Option<impl IntoElement + use<>> {
    art.map(|art| {
        img(art.clone())
            .size(px(side))
            .flex_shrink_0()
            .rounded(px(radius))
            .object_fit(ObjectFit::Fill)
    })
}

/// A media button `size` big, its glyph `glyph_side` square. Pressing it asks the session's app
/// from the media thread; the icon changes when the app reports back. A button the app doesn't
/// take is greyed and does nothing.
pub(super) fn control(
    button: Button,
    glyph: Icon,
    enabled: bool,
    size: (f32, f32),
    glyph_side: f32,
) -> Stateful<Div> {
    let base = div()
        .id(("media", button as usize))
        .w(px(size.0))
        .h(px(size.1))
        .flex_shrink_0()
        .rounded(px(10.0))
        .flex()
        .items_center()
        .justify_center()
        .child(icons::icon(glyph, glyph_side, rgb(GLYPH)));
    if !enabled {
        return base.opacity(0.3);
    }
    base.cursor_pointer()
        .hover(|style| style.bg(rgba(0xffffff14)))
        .on_click(move |_: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            log(&format!("media press {button:?}"));
            media::press(button);
        })
}

/// Play or pause, as the track is now.
pub(super) fn play_pause_glyph(playing: bool) -> Icon {
    if playing { Icon::Pause } else { Icon::Play }
}
