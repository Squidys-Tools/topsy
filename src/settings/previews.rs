//! Little pictures of the island over a wallpaper, drawn from the settings as they are, so a
//! change shows in the window before it is seen on the notch.

use gpui::{Div, div, linear_color_stop, linear_gradient, prelude::*, px, rgb, rgba};

use crate::icons::{self, Icon};

/// A warm wallpaper for the notch to hang over.
fn wallpaper(height: f32) -> Div {
    div()
        .relative()
        .w_full()
        .h(px(height))
        .overflow_hidden()
        .rounded(px(10.0))
        .bg(linear_gradient(
            120.0,
            linear_color_stop(rgb(0x3b2140), 0.0),
            linear_color_stop(rgb(0xd0603c), 1.0),
        ))
}

/// The notch as the island draws it: black, or glass over the wallpaper.
fn notch(black: bool, radius: f32) -> Div {
    let notch = div().flex().rounded_b(px(radius));
    if black {
        notch.bg(rgb(0x000000))
    } else {
        notch
            .bg(rgba(0x1e1e22a6))
            .border_1()
            .border_t_0()
            .border_color(rgba(0xffffff26))
    }
}

/// Hung from the middle of the top edge.
fn hung(notch: Div) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .w_full()
        .flex()
        .justify_center()
        .child(notch)
}

/// The tray, open: a few files on the shelf and the widgets that are on, as they line up.
pub(super) fn shelf(black: bool, widgets: &[(Icon, u32)]) -> Div {
    let chip = || div().size(px(34.0)).rounded(px(7.0)).bg(rgba(0xffffff1f));
    let widgets = widgets.iter().map(|&(icon, tint)| {
        div()
            .size(px(34.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(7.0))
            .bg(rgb(tint))
            .child(icons::icon(icon, 16.0, rgb(0xffffff)))
    });
    wallpaper(84.0).child(hung(
        notch(black, 18.0)
            .gap(px(6.0))
            .px(px(14.0))
            .py(px(10.0))
            .children((0..3).map(|_| chip()))
            .children(widgets),
    ))
}

/// The clipboard palette, hanging with a few entries in it.
pub(super) fn clipboard(black: bool) -> Div {
    let entries = [0.9, 0.7, 0.8, 0.5].map(|width| {
        div()
            .h(px(10.0))
            .w(gpui::relative(width))
            .rounded(px(4.0))
            .bg(rgba(0xffffff26))
    });
    wallpaper(84.0).child(hung(
        notch(black, 14.0)
            .flex_col()
            .gap(px(5.0))
            .w(px(200.0))
            .p(px(9.0))
            .children(entries),
    ))
}

/// The notch alone, small: a picture for the choice between glass and black.
pub(super) fn look(black: bool) -> Div {
    wallpaper(52.0)
        .rounded(px(7.0))
        .child(hung(notch(black, 10.0).w(px(90.0)).h(px(22.0))))
}
