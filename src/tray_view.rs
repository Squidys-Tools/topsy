//! The tray's layout: Now Playing's player when the tray was opened to look, then the shelf's
//! header and its chips (see `TrayCause::sections` for which show when).

use gpui::{ClickEvent, Context, ScrollHandle, Window, div, prelude::*, px, rgb, rgba};

use crate::model::tray_cause::Sections;
use crate::{Island, Mode, now_playing};

/// Between chips.
const GAP: f32 = 8.0;

impl Island {
    pub(crate) fn sections(&self) -> Sections {
        let playing = self.media.now.card().is_some();
        self.tray_cause.sections(playing, self.shelf.is_empty())
    }

    /// The notch body's size for `mode`; the tray's height is its sections'.
    pub(crate) fn size_for(&self, mode: Mode) -> (f32, f32) {
        let (w, h) = mode.size();
        if mode == Mode::Peek && self.hud.is_shown() {
            return crate::hud::SIZE;
        }
        if mode != Mode::Tray {
            return (w, h);
        }
        let sections = self.sections();
        let player = if sections.media {
            now_playing::HEIGHT
        } else {
            0.0
        };
        let shelf = if sections.shelf { h } else { 0.0 };
        (w, player + shelf)
    }

    /// The open tray springs to its sections' height when they change under it: the track
    /// stops, the first file lands.
    pub(crate) fn fit_tray(&mut self) {
        if self.mode == Mode::Tray {
            self.height.set_target(self.size_for(Mode::Tray).1);
        }
    }

    /// The expanded tray: the player and the shelf, as its sections say.
    pub(crate) fn tray_content(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let sections = self.sections();
        // Fixed size and centred in the window, so it is revealed (not squashed) while the
        // pill springs open.
        let (w, h) = self.size_for(Mode::Tray);
        div()
            .w(px(w))
            .h(px(h))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .relative()
            .children(sections.media.then(|| self.player(cx)).flatten())
            .children(sections.shelf.then(|| self.shelf_section(window, cx)))
            .children(self.hud_over_tray())
    }

    /// The shelf's header and its chips, or the hint when it is empty.
    fn shelf_section(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
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
        let files: Vec<_> = (self.shelf.items().iter())
            .filter(|item| item.kind == crate::model::shelf::Kind::File)
            .map(|item| item.path.clone())
            .collect();
        let transform = (!files.is_empty()).then(|| {
            div()
                .id("transform")
                .px(px(6.0))
                .rounded(px(4.0))
                .text_xs()
                .text_color(rgba(0xffffffb3))
                .cursor_pointer()
                .hover(|style| style.bg(rgba(0xffffff1a)).text_color(rgb(0xffffff)))
                .child("Transform")
                .on_click(cx.listener(move |_, _: &ClickEvent, _window, cx| {
                    let island = cx.entity();
                    crate::transform::open(cx, &island, files.clone());
                }))
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
            let scroll = (window.use_keyed_state("chips", cx, |_, _| ScrollHandle::new()))
                .read(cx)
                .clone();
            // A chip cut by the row's edge fades with how much of it is out of sight, so a full
            // shelf shows there is more past the edge. Scrolling re-renders the tray.
            let (x, row) = (
                f32::from(scroll.offset().x),
                f32::from(scroll.bounds().size.width),
            );
            let chips: Vec<_> = (self.shelf.items().iter().enumerate())
                .map(|(i, item)| {
                    let left = i as f32 * (crate::chip::CHIP.0 + GAP) + x;
                    let right = left + crate::chip::CHIP.0;
                    let seen = (right.min(row) - left.max(0.0)).max(0.0) / crate::chip::CHIP.0;
                    let opacity = if row <= 0.0 || seen >= 1.0 {
                        1.0
                    } else {
                        0.15 + 0.5 * seen
                    };
                    div().opacity(opacity).child(self.chip(i, item, cx))
                })
                .collect();
            div()
                .id("chips")
                .w_full()
                .flex()
                .flex_row()
                .gap(px(GAP))
                .overflow_x_scroll()
                .track_scroll(&scroll)
                .children(chips)
                .into_any_element()
        };

        let (_, h) = Mode::Tray.size();
        div()
            .w_full()
            .h(px(h))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .p(px(10.0))
            .gap(px(6.0))
            .child(
                div()
                    .w_full()
                    .h(px(16.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(div().text_xs().text_color(rgba(0xffffffb3)).child(title))
                    .child(
                        div()
                            .flex()
                            .gap(px(4.0))
                            .children(transform)
                            .children(clear),
                    ),
            )
            .child(body)
    }
}
