//! The settings page's About: the version, the update waiting, and where to read more.

use std::path::Path;

use gpui::{Div, FontWeight, div, prelude::*, px, rgb, rgba};

use super::controls::{DIM, HAIRLINE, button, grid, row};
use crate::island::chip::on_thread;
use crate::{log, update, win32};

const SOURCE: &str = env!("CARGO_PKG_REPOSITORY");
const HELP: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/tree/main/docs/user");

pub(super) fn about() -> Div {
    let ready = update::ready();
    let version = match &ready {
        Some(nightly) => format!("Version {} · {} is ready", update::VERSION, nightly.short()),
        None => format!("Version {}", update::VERSION),
    };
    let badge = div()
        .size(px(40.0))
        .flex_none()
        .flex()
        .justify_center()
        .rounded(px(11.0))
        .bg(rgb(0x15151a))
        .border_1()
        .border_color(rgba(0xffffff29))
        .child(
            div()
                .w(px(22.0))
                .h(px(10.0))
                .mt(px(9.0))
                .rounded_b(px(7.0))
                .bg(rgb(0xffffff)),
        );
    let restart = ready.map(|_| {
        button("settings-restart", "Restart to update", true).on_click(|_, _, cx| {
            cx.stop_propagation();
            update::restart(cx);
        })
    });
    let header = div()
        .flex()
        .items_center()
        .gap(px(14.0))
        .pt(px(4.0))
        .pb(px(10.0))
        .border_b_1()
        .border_color(rgba(HAIRLINE))
        .child(badge)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(14.0))
                        .font_weight(FontWeight::BOLD)
                        .child("topsy"),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgba(DIM))
                        .truncate()
                        .child(version),
                ),
        )
        .children(restart);
    let link = |id: &'static str, label: &'static str, description: &'static str, url| {
        let open = button(id, "Open", false).on_click(move |_, _, cx| {
            cx.stop_propagation();
            open_url(url);
        });
        row(label, description, open)
    };
    div().flex().flex_col().child(header).child(grid(vec![
        link("settings-source", "Source code", "On GitHub", SOURCE),
        link(
            "settings-help",
            "Help",
            "What each part of topsy does",
            HELP,
        ),
    ]))
}

/// `ShellExecute` pumps messages: off the UI thread.
fn open_url(url: &'static str) {
    on_thread("topsy-open-url", move || {
        if !win32::open_path(Path::new(url)) {
            log(&format!("open {url} failed"));
        }
    });
}
