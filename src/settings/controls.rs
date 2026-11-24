//! The settings window's pieces, in the island's own look: white on glass, hairlines between
//! rows, pills for choices, and no accent colour beyond white.

use gpui::{
    AnyElement, Div, ElementId, FontWeight, SharedString, Stateful, div, prelude::*, px, rgb, rgba,
};

pub(super) const TEXT: u32 = 0xffffff;
pub(super) const DIM: u32 = 0xffffff8c;
pub(super) const HOVER: u32 = 0xffffff0d;
const LINE: u32 = 0xffffff14;

/// A page's title and the line under it, at the top of its content.
pub(super) fn title(text: &'static str, line: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .pb(px(16.0))
        .child(
            div()
                .text_size(px(22.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(text),
        )
        .child(
            div()
                .text_size(px(13.0))
                .text_color(rgba(DIM))
                .child(line.into()),
        )
}

/// A run of rows under an optional heading, each with a hairline above it.
pub(super) fn card(heading: Option<&'static str>, rows: Vec<AnyElement>) -> Div {
    let rows = rows
        .into_iter()
        .map(|row| div().border_t_1().border_color(rgba(LINE)).child(row));
    div()
        .flex()
        .flex_col()
        .pb(px(18.0))
        .children(heading.map(|heading| {
            div()
                .pb(px(6.0))
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgba(DIM))
                .child(heading.to_uppercase())
        }))
        .children(rows)
}

/// A row: its label and a dimmer line under it on the left, `control` on the right.
pub(super) fn row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    description: Option<&'static str>,
    control: impl IntoElement,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(16.0))
        .min_h(px(if description.is_some() { 58.0 } else { 46.0 }))
        .px(px(10.0))
        .mx(px(-10.0))
        .py(px(8.0))
        .rounded(px(10.0))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(2.0))
                .child(div().text_size(px(14.0)).child(label.into()))
                .children(
                    description
                        .map(|line| div().text_size(px(12.0)).text_color(rgba(DIM)).child(line)),
                ),
        )
        .child(control)
}

/// A switch: white when on with a dark knob at the right, faint when off.
pub(super) fn switch(on: bool) -> Div {
    let (track, knob) = if on {
        (rgb(0xffffff), rgb(0x111111))
    } else {
        (rgba(0xffffff29), rgba(0xffffffb3))
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .when(on, |track| track.justify_end())
        .w(px(38.0))
        .h(px(22.0))
        .px(px(3.0))
        .rounded_full()
        .bg(track)
        .child(div().size(px(16.0)).rounded_full().bg(knob))
}

/// A pill button; `primary` fills it white for the one action a row is for.
pub(super) fn button(
    id: impl Into<ElementId>,
    label: &'static str,
    primary: bool,
) -> Stateful<Div> {
    let base = div()
        .id(id)
        .flex_none()
        .px(px(14.0))
        .py(px(5.0))
        .rounded_full()
        .text_size(px(13.0))
        .child(label);
    if primary {
        base.bg(rgb(0xffffff))
            .text_color(rgb(0x111111))
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|button| button.opacity(0.85))
    } else {
        base.bg(rgba(0xffffff1a))
            .hover(|button| button.bg(rgba(0xffffff29)))
    }
}

/// Pills side by side, the chosen one white; `on_pick` gets the picked pill's index.
pub(super) fn segmented(
    id: &'static str,
    labels: impl IntoIterator<Item = String>,
    chosen: Option<usize>,
    on_pick: impl Fn(usize, &mut gpui::Window, &mut gpui::App) + Clone + 'static,
) -> Div {
    let pills = labels.into_iter().enumerate().map(|(i, label)| {
        let on_pick = on_pick.clone();
        let picked = chosen == Some(i);
        div()
            .id((id, i))
            .px(px(10.0))
            .py(px(4.0))
            .rounded_full()
            .text_size(px(12.0))
            .when(picked, |pill| {
                pill.bg(rgb(0xffffff))
                    .text_color(rgb(0x111111))
                    .font_weight(FontWeight::SEMIBOLD)
            })
            .when(!picked, |pill| {
                pill.bg(rgba(0xffffff1a))
                    .hover(|pill| pill.bg(rgba(0xffffff29)))
            })
            .on_click(move |_, window, cx| on_pick(i, window, cx))
            .child(label)
    });
    div().flex().flex_none().gap(px(6.0)).children(pills)
}
