//! The shelf's page, after Droppy's File Tray: a row of round icon buttons over the chips. The
//! select button on the left starts picking, and once chips are picked brings out what acts on
//! them; the buttons on the right act on the picked chips, or on the whole shelf when none are.

use std::path::PathBuf;

use gpui::{
    ClickEvent, Context, Div, MouseButton, MouseDownEvent, Stateful, div, prelude::*, px, rgba,
};

use crate::Island;
use crate::frame::TRAY;
use crate::icons::{Icon, icon};
use crate::island::chip::{self, ACCENT, CHIP};
use model::shelf::{Item, Kind};

/// Between chips.
const GAP: f32 = 6.0;
/// A header button, round.
const BUTTON: f32 = 28.0;
const PAD: f32 = 10.0;

/// What a header button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Button {
    Select,
    Compress,
    ShowInFolder,
    Transform,
    Basket,
    Remove,
}

impl Button {
    fn icon(self) -> Icon {
        match self {
            Button::Select => Icon::CheckCircle,
            Button::Compress => Icon::Archive,
            Button::ShowInFolder => Icon::FolderExport,
            Button::Transform => Icon::Convert,
            Button::Basket => Icon::Basket,
            Button::Remove => Icon::Delete,
        }
    }
}

impl Island {
    /// The header's buttons and the chips, or the hint when the shelf is empty.
    pub(super) fn shelf_section(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let picking = self.shelf.is_picking();
        let picked = !self.shelf.selection().is_empty();
        let targets = self.shelf.picked_or_all();
        let has_files = targets.iter().any(|item| item.kind == Kind::File);
        let left = (!self.shelf.is_empty()).then(|| {
            let mut buttons = vec![self.button(Button::Select, picking, cx)];
            if picked {
                buttons.push(self.button(Button::Compress, false, cx));
                buttons.push(self.button(Button::ShowInFolder, false, cx));
            }
            buttons
        });
        let right = (!self.shelf.is_empty()).then(|| {
            let mut buttons = Vec::new();
            if has_files {
                buttons.push(self.button(Button::Transform, false, cx));
            }
            buttons.push(self.button(Button::Basket, false, cx));
            buttons.push(self.button(Button::Remove, false, cx));
            buttons
        });
        let row = |buttons: Option<Vec<Stateful<Div>>>| {
            div()
                .flex()
                .gap(px(6.0))
                .children(buttons.into_iter().flatten())
        };
        let header = div()
            .w_full()
            .h(px(BUTTON))
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(row(left))
            .child(row(right));

        let body = if self.shelf.is_empty() {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .text_sm()
                .text_color(rgba(0xffffff99))
                .child(icon(Icon::Download, 24.0, rgba(0xffffff80)))
                .child("Drop files here")
                .into_any_element()
        } else {
            self.chips(cx).into_any_element()
        };

        let (_, h) = TRAY;
        div()
            .id("shelf")
            .w_full()
            .h(px(h))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .px(px(PAD))
            .pt(px(PAD))
            .gap(px(6.0))
            .child(header)
            .child(body)
            // A chip's click stops here; one anywhere else on the shelf lets the selection go.
            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                if this.shelf.is_picking() {
                    this.shelf.deselect();
                    cx.notify();
                }
            }))
    }

    /// The chips in a row that scrolls sideways. A chip cut by the row's edge fades with how much
    /// of it is out of sight, so a full shelf shows there is more past the edge. Scrolling
    /// re-renders the tray.
    fn chips(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let scroll = self.pager.chips.clone();
        let (x, row) = (
            f32::from(scroll.offset().x),
            f32::from(scroll.bounds().size.width),
        );
        let chips: Vec<_> = (self.shelf.items().iter().enumerate())
            .map(|(i, item)| {
                let left = i as f32 * (CHIP.0 + GAP) + x;
                let right = left + CHIP.0;
                let seen = (right.min(row) - left.max(0.0)).max(0.0) / CHIP.0;
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
    }

    /// A round icon button in the header; `on` lights it in the accent (the select button while
    /// picking).
    fn button(&self, button: Button, on: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let (fill, color) = match on {
            true => (rgba(ACCENT << 8 | 0x38), rgba(0x5eb0ffff)),
            false => (rgba(0xffffff14), rgba(0xffffffd9)),
        };
        div()
            .id(("shelf-button", button as usize))
            .size(px(BUTTON))
            .flex_shrink_0()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(fill)
            .cursor_pointer()
            .hover(|style| style.bg(rgba(0xffffff2e)))
            .child(icon(button.icon(), 16.0, color))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _: &MouseDownEvent, _window, cx| cx.stop_propagation()),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                cx.stop_propagation();
                this.shelf_button(button, cx);
            }))
    }

    fn shelf_button(&mut self, button: Button, cx: &mut Context<Self>) {
        let targets = self.shelf.picked_or_all();
        let paths = |items: &[Item]| -> Vec<PathBuf> {
            items.iter().map(|item| item.path.clone()).collect()
        };
        match button {
            Button::Select => self.shelf.toggle_picking(),
            Button::Compress => chip::zip(paths(&targets), cx, |this: &mut Self, zip, cx| {
                this.add_paths(vec![zip], cx);
            }),
            Button::ShowInFolder => {
                if let Some(first) = targets.into_iter().next() {
                    chip::show_in_folder(first.path);
                }
            }
            Button::Transform => {
                let files: Vec<Item> = (targets.into_iter())
                    .filter(|item| item.kind == Kind::File)
                    .collect();
                let island = cx.entity();
                crate::transform::open(cx, &island, paths(&files));
            }
            Button::Basket => {
                if let Some(basket) = self.basket.clone() {
                    self.take_off_shelf(&targets, cx);
                    let paths = paths(&targets);
                    basket.update(cx, |basket, cx| basket.take(paths, cx));
                }
            }
            Button::Remove => self.take_off_shelf(&targets, cx),
        }
        cx.notify();
    }

    fn take_off_shelf(&mut self, items: &[Item], cx: &mut Context<Self>) {
        for item in items {
            self.shelf.remove_path(&item.path);
        }
        // What was picked has gone: the bin must not go on to empty the rest.
        self.shelf.deselect();
        self.shelf_changed(cx);
    }
}
