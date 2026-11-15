//! The tray's layout: what is playing, if anything, then the shelf's header and its chips.

use gpui::{ClickEvent, Context, div, prelude::*, px, rgb, rgba};

use crate::{Island, Mode, now_playing};

impl Island {
    /// The notch body's size for `mode`; the tray grows by Now Playing's row while it shows.
    pub(crate) fn size_for(&self, mode: Mode) -> (f32, f32) {
        let (w, h) = mode.size();
        if mode == Mode::Tray && self.media.card().is_some() {
            (w, h + now_playing::ROW)
        } else {
            (w, h)
        }
    }

    /// The expanded tray: Now Playing, a header and a row of chips (or the hint, when the
    /// shelf is empty).
    pub(crate) fn tray_content(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let count = self.shelf.len();
        let title = if count == 0 {
            "Shelf".to_string()
        } else {
            format!("Shelf · {count}")
        };
        let clear = (count > 0).then(|| {
            div()
                .id("clear")
                .px(px(6.0))
                .rounded(px(4.0))
                .text_xs()
                .text_color(rgba(0xffffffb3))
                .cursor_pointer()
                .hover(|style| style.bg(rgba(0xffffff1a)).text_color(rgb(0xffffff)))
                .child("Clear")
                .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| this.clear_shelf(cx)))
        });

        let body = if self.shelf.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .child("Drop files to park them")
                .into_any_element()
        } else {
            let chips: Vec<_> = self
                .shelf
                .items()
                .iter()
                .enumerate()
                .map(|(i, item)| self.chip(i, item, cx))
                .collect();
            div()
                .id("chips")
                .w_full()
                .flex()
                .flex_row()
                .gap(px(8.0))
                .overflow_x_scroll()
                .children(chips)
                .into_any_element()
        };

        // Fixed size and centred in the window, so it is revealed (not squashed) while the
        // pill springs open.
        let (w, h) = self.size_for(Mode::Tray);
        div()
            .w(px(w))
            .h(px(h))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .p(px(10.0))
            .gap(px(6.0))
            .children(self.media_row(cx))
            .child(
                div()
                    .w_full()
                    .h(px(16.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(div().text_xs().text_color(rgba(0xffffffb3)).child(title))
                    .children(clear),
            )
            .child(body)
    }
}
