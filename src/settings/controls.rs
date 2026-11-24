//! The settings page's controls, drawn as the island draws: white on the dark glass.

use gpui::{
    ClickEvent, Context, Div, FontWeight, MouseDownEvent, Stateful, Window, div, prelude::*, px,
    rgb, rgba,
};

use super::page::{Effect, State};
use crate::Island;

pub(super) const DIM: u32 = 0xffffff80;
pub(super) const HAIRLINE: u32 = 0xffffff12;
pub(super) const FILL: u32 = 0xffffff1a;
pub(super) const FILL_HOVER: u32 = 0xffffff2e;

/// A press on the page stays on it: the island under it neither starts a drag nor opens.
pub(super) fn keep_press(
    _: &mut Island,
    _: &MouseDownEvent,
    _: &mut Window,
    cx: &mut Context<Island>,
) {
    cx.stop_propagation();
}

/// Make `element` a button that changes the page's state with `change`.
pub(super) fn pressable(
    element: Stateful<Div>,
    cx: &mut Context<Island>,
    change: impl Fn(&mut State) -> Option<Effect> + 'static,
) -> Stateful<Div> {
    element
        .cursor_pointer()
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            cx.stop_propagation();
            this.update_settings(cx, |state, _| change(state));
        }))
}

/// A label with a dim line under it, and its control on the right.
pub(super) fn row(label: &str, description: &'static str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .py(px(6.0))
        .min_h(px(30.0))
        .border_b_1()
        .border_color(rgba(HAIRLINE))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().truncate().child(label.to_owned()))
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(rgba(DIM))
                        .truncate()
                        .child(description),
                ),
        )
        .child(control)
}

/// `rows` two to a line.
pub(super) fn grid(rows: Vec<Div>) -> Div {
    let mut lines = Vec::new();
    let mut rows = rows.into_iter();
    while let Some(left) = rows.next() {
        let right = rows.next().unwrap_or_else(div);
        lines.push(
            div()
                .flex()
                .gap(px(20.0))
                .child(left.flex_1().min_w_0())
                .child(right.flex_1().min_w_0()),
        );
    }
    div().flex().flex_col().children(lines)
}

pub(super) fn switch(on: bool) -> Div {
    let (track, knob) = match on {
        true => (rgba(0xffffffff), rgba(0x000000ff)),
        false => (rgba(0xffffff33), rgba(0xffffffd9)),
    };
    div()
        .w(px(28.0))
        .h(px(16.0))
        .flex_none()
        .flex()
        .items_center()
        .px(px(2.0))
        .rounded_full()
        .bg(track)
        .when(on, |track| track.justify_end())
        .child(div().size(px(12.0)).rounded_full().bg(knob))
}

pub(super) fn button(id: &'static str, label: &'static str, primary: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .px(px(10.0))
        .py(px(3.0))
        .rounded_full()
        .text_size(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .map(|button| match primary {
            true => button.bg(rgb(0xffffff)).text_color(rgb(0x000000)),
            false => button
                .bg(rgba(FILL))
                .hover(|button| button.bg(rgba(FILL_HOVER))),
        })
        .child(label)
}

pub(super) fn dot() -> Div {
    div().size(px(5.0)).rounded_full().bg(rgb(0xffffff))
}
