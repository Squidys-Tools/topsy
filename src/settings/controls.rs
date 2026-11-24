//! The settings window's pieces, after Droppy's: grouped rows on a dark pane, blue for what is on
//! or chosen, pills and tiles for choices.

use gpui::{
    AnyElement, Div, ElementId, FontWeight, Hsla, SharedString, Stateful, div, prelude::*, px, rgb,
    rgba,
};

use crate::icons::{self, Icon};

pub(super) const TEXT: u32 = 0xffffff;
pub(super) const DIM: u32 = 0xffffff8c;
pub(super) const BLUE: u32 = 0x1f7aff;
pub(super) const LINE: u32 = 0xffffff14;
const FILL: u32 = 0xffffff0e;
const RAISED: u32 = 0xffffff1a;
const RAISED_HOVER: u32 = 0xffffff2b;

/// A page's title, at the top of its content.
pub(super) fn title(text: &'static str) -> Div {
    div()
        .pb(px(4.0))
        .text_size(px(20.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

/// A rounded box of `rows`, hairlines between them, under an optional heading.
pub(super) fn section(heading: Option<&'static str>, rows: Vec<AnyElement>) -> Div {
    let rows = rows.into_iter().enumerate().map(|(i, row)| {
        div()
            .when(i > 0, |row| row.border_t_1().border_color(rgba(LINE)))
            .child(row)
    });
    div()
        .flex()
        .flex_col()
        .pt(px(14.0))
        .children(heading.map(|heading| {
            div()
                .pb(px(6.0))
                .pl(px(4.0))
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgba(DIM))
                .child(heading.to_uppercase())
        }))
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(px(12.0))
                .bg(rgba(FILL))
                .border_1()
                .border_color(rgba(LINE))
                .children(rows),
        )
}

/// A row: its label and a dimmer line under it on the left, `control` on the right.
pub(super) fn row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    description: &'static str,
    control: impl IntoElement,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(12.0))
        .px(px(14.0))
        .py(px(10.0))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(div().text_size(px(13.0)).child(label.into()))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(rgba(DIM))
                        .child(description),
                ),
        )
        .child(control)
}

/// A switch, blue with its knob at the right when on.
pub(super) fn switch(on: bool) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .when(on, |track| track.justify_end().bg(rgb(BLUE)))
        .when(!on, |track| track.bg(rgba(0xffffff33)))
        .w(px(36.0))
        .h(px(20.0))
        .px(px(2.0))
        .rounded_full()
        .child(div().size(px(16.0)).rounded_full().bg(rgb(TEXT)))
}

/// A small button; `primary` makes it blue, for the one action a row is for.
pub(super) fn button(
    id: impl Into<ElementId>,
    label: &'static str,
    primary: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .px(px(12.0))
        .py(px(4.0))
        .rounded(px(8.0))
        .text_size(px(12.0))
        .when(primary, |button| {
            button
                .bg(rgb(BLUE))
                .font_weight(FontWeight::SEMIBOLD)
                .hover(|button| button.opacity(0.85))
        })
        .when(!primary, |button| {
            button
                .bg(rgba(RAISED))
                .hover(|button| button.bg(rgba(RAISED_HOVER)))
        })
        .child(label)
}

/// A wide pill for the sidebar: an icon and a label, blue when `on`.
pub(super) fn pill(
    id: impl Into<ElementId>,
    icon: Option<Icon>,
    label: &'static str,
    on: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(10.0))
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(14.0))
        .border_1()
        .text_size(px(13.0))
        .font_weight(FontWeight::SEMIBOLD)
        .when(on, |pill| pill.bg(rgb(BLUE)).border_color(rgba(0xffffff40)))
        .when(!on, |pill| {
            pill.bg(rgba(0xffffff12))
                .border_color(rgba(0xffffff24))
                .hover(|pill| pill.bg(rgba(0xffffff21)))
        })
        .children(icon.map(|icon| icons::icon(icon, 16.0, rgb(TEXT))))
        .child(label)
}

/// Options side by side in a sunken strip, the chosen one raised; `on_pick` gets its index.
pub(super) fn segmented(
    id: &'static str,
    labels: impl IntoIterator<Item = String>,
    chosen: Option<usize>,
    on_pick: impl Fn(usize, &mut gpui::Window, &mut gpui::App) + Clone + 'static,
) -> Div {
    let options = labels.into_iter().enumerate().map(|(i, label)| {
        let on_pick = on_pick.clone();
        let picked = chosen == Some(i);
        div()
            .id((id, i))
            .px(px(9.0))
            .py(px(3.0))
            .rounded(px(6.0))
            .text_size(px(12.0))
            .when(picked, |option| {
                option
                    .bg(rgba(0xffffff38))
                    .font_weight(FontWeight::SEMIBOLD)
            })
            .when(!picked, |option| {
                option.hover(|option| option.bg(rgba(0xffffff14)))
            })
            .on_click(move |_, window, cx| on_pick(i, window, cx))
            .child(label)
    });
    div()
        .flex()
        .flex_none()
        .p(px(2.0))
        .rounded(px(8.0))
        .bg(rgba(0xffffff14))
        .children(options)
}

/// A tile for a feature of its own: an icon in a rounded square, a name and a line, blue when on.
pub(super) fn tile(
    id: impl Into<ElementId>,
    icon: Icon,
    label: impl Into<SharedString>,
    description: &'static str,
    on: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .p(px(12.0))
        .rounded(px(12.0))
        .border_1()
        .when(on, |tile| {
            tile.bg(rgba(0x1f7aff38)).border_color(rgba(0x1f7affb3))
        })
        .when(!on, |tile| {
            tile.bg(rgba(0xffffff0f))
                .border_color(rgba(LINE))
                .hover(|tile| tile.bg(rgba(0xffffff17)))
        })
        .child(
            div()
                .size(px(30.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(9.0))
                .bg(if on { rgb(BLUE) } else { rgba(0xffffff1f) })
                .child(icons::icon(icon, 16.0, rgb(TEXT))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(label.into()),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgba(DIM))
                        .child(description),
                ),
        )
}

/// A choice shown as a picture over its name, ringed blue when chosen.
pub(super) fn choice(
    id: impl Into<ElementId>,
    picture: impl IntoElement,
    label: &'static str,
    chosen: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.0))
        .p(px(8.0))
        .rounded(px(10.0))
        .border_2()
        .bg(rgba(0xffffff0d))
        .border_color(if chosen {
            Hsla::from(rgb(BLUE))
        } else {
            Hsla::transparent_black()
        })
        .when(!chosen, |choice| {
            choice.hover(|choice| choice.bg(rgba(0xffffff17)))
        })
        .text_size(px(12.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child(picture)
        .child(label)
}
