//! The settings window's pages and what each one holds. Every [`Setting`] has a place, so a new
//! setting is a compile error here until it is given one.

use std::path::Path;

use gpui::{AnyElement, Context, Div, FontWeight, div, prelude::*, px, rgb, rgba};

use super::controls::{DIM, TEXT, button, choice, row, section, segmented, switch, tile, title};
use super::previews;
use super::window::SettingsWindow;
use crate::icons::{self, Icon};
use crate::log::LogErr;
use crate::settings::Setting;
use crate::{clipboard, update, win32};

const REPO: &str = "https://github.com/Squidys-Tools/topsy";
const DOCS: &str = "https://github.com/Squidys-Tools/topsy/tree/main/docs/user";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Shelf,
    Notch,
    Clipboard,
    General,
    About,
}

impl Page {
    /// In the order the sidebar lists them.
    pub(super) const ALL: [Page; 5] = [
        Page::Shelf,
        Page::Notch,
        Page::Clipboard,
        Page::General,
        Page::About,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            Page::Shelf => "Shelf & Widgets",
            Page::Notch => "Notch & HUD",
            Page::Clipboard => "Clipboard",
            Page::General => "General",
            Page::About => "About",
        }
    }

    pub(super) fn icon(self) -> Icon {
        match self {
            Page::Shelf => Icon::Tray,
            Page::Notch => Icon::VolumeHigh,
            Page::Clipboard => Icon::Copy,
            Page::General => Icon::Settings,
            Page::About => Icon::Info,
        }
    }
}

/// How a setting is shown: a row with a switch, a tile, or the glass-or-black pictures.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Shelf,
    Look,
    /// A tile with this icon.
    Tile(Icon),
    Clipboard,
    General,
}

fn place(setting: Setting) -> Place {
    match setting {
        Setting::SendTo => Place::Shelf,
        Setting::Black | Setting::LiveGlass => Place::Look,
        Setting::VolumeHud => Place::Tile(Icon::VolumeHigh),
        Setting::LiveActivities => Place::Tile(Icon::Download),
        Setting::Clipboard => Place::Clipboard,
        Setting::Autostart | Setting::Updates => Place::General,
    }
}

fn days(days: u32) -> String {
    match days {
        0 => "Forever".into(),
        1 => "1 day".into(),
        n => format!("{n} days"),
    }
}

/// Open a web page in the browser, off the UI thread, since ShellExecute pumps messages.
fn open_link(url: &'static str) {
    std::thread::spawn(move || {
        if !win32::open_path(Path::new(url)) {
            eprintln!("topsy: could not open {url}");
        }
    });
}

impl SettingsWindow {
    pub(super) fn page(&self, page: Page, cx: &mut Context<Self>) -> Div {
        let body = div().flex().flex_col().pb(px(18.0));
        let black = self.is(Setting::Black);
        match page {
            Page::Shelf => {
                let preview = div()
                    .p(px(10.0))
                    .child(previews::shelf(black, &self.shown_widgets()))
                    .into_any_element();
                let rows = std::iter::once(preview).chain(self.rows(Place::Shelf, cx));
                body.child(title(page.name()))
                    .child(section(None, rows.collect()))
                    .child(self.widgets(cx))
            }
            Page::Notch => body
                .child(title(page.name()))
                .child(section(Some("Appearance"), self.look(cx)))
                .child(section(Some("In the notch"), vec![self.tiles(cx)])),
            Page::Clipboard => {
                let preview = div()
                    .p(px(10.0))
                    .child(previews::clipboard(black))
                    .into_any_element();
                let mut rows = self.rows(Place::Clipboard, cx);
                rows.insert(1.min(rows.len()), preview);
                rows.extend(self.retention(cx));
                body.child(title(page.name())).child(section(None, rows))
            }
            Page::General => body
                .child(title(page.name()))
                .child(section(None, self.rows(Place::General, cx))),
            Page::About => body.child(self.about()),
        }
    }

    fn is(&self, setting: Setting) -> bool {
        (Setting::ALL.iter().position(|kept| *kept == setting)).is_some_and(|i| self.on[i])
    }

    /// The settings in `which`, with their index in [`Setting::ALL`].
    fn placed(which: Place) -> impl Iterator<Item = (usize, Setting)> {
        (Setting::ALL.into_iter().enumerate()).filter(move |(_, setting)| place(*setting) == which)
    }

    /// A row with a switch for each setting in `which`.
    fn rows(&self, which: Place, cx: &mut Context<Self>) -> Vec<AnyElement> {
        Self::placed(which)
            .map(|(i, setting)| {
                row(
                    ("setting", i),
                    setting.label(),
                    setting.description(),
                    switch(self.on[i]),
                )
                .on_click(cx.listener(move |view, _, _, cx| view.toggle(i, setting, cx)))
                .into_any_element()
            })
            .collect()
    }

    /// Glass or black as two pictures to pick from, then the rest of the look as rows.
    fn look(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows = Vec::new();
        for (i, setting) in Self::placed(Place::Look) {
            match setting {
                Setting::Black => {
                    let black = self.on[i];
                    let pick = |want: bool| {
                        cx.listener(move |view: &mut Self, _: &gpui::ClickEvent, _, cx| {
                            if view.on[i] != want {
                                view.toggle(i, setting, cx);
                            }
                        })
                    };
                    let glass = choice("glass", previews::look(false), "Glass", !black)
                        .on_click(pick(false));
                    let solid =
                        choice("black", previews::look(true), "Black", black).on_click(pick(true));
                    rows.push(
                        div()
                            .flex()
                            .gap(px(10.0))
                            .p(px(12.0))
                            .child(glass)
                            .child(solid)
                            .into_any_element(),
                    );
                }
                Setting::LiveGlass
                | Setting::Autostart
                | Setting::SendTo
                | Setting::Clipboard
                | Setting::VolumeHud
                | Setting::LiveActivities
                | Setting::Updates => rows.push(
                    row(
                        ("setting", i),
                        setting.label(),
                        setting.description(),
                        switch(self.on[i]),
                    )
                    .on_click(cx.listener(move |view, _, _, cx| view.toggle(i, setting, cx)))
                    .into_any_element(),
                ),
            }
        }
        rows
    }

    fn tiles(&self, cx: &mut Context<Self>) -> AnyElement {
        let tiles = Setting::ALL
            .into_iter()
            .enumerate()
            .filter_map(|(i, setting)| {
                let Place::Tile(icon) = place(setting) else {
                    return None;
                };
                let tile = tile(
                    ("setting", i),
                    icon,
                    setting.label(),
                    setting.description(),
                    self.on[i],
                )
                .on_click(cx.listener(move |view, _, _, cx| view.toggle(i, setting, cx)));
                Some(tile)
            });
        div()
            .flex()
            .gap(px(10.0))
            .p(px(12.0))
            .children(tiles)
            .into_any_element()
    }

    fn retention(&self, cx: &mut Context<Self>) -> [AnyElement; 2] {
        let choices = clipboard::DAY_CHOICES;
        let chosen = choices.iter().position(|&kept| kept == self.keep_days);
        let window = cx.entity().downgrade();
        let pick = move |i: usize, _: &mut gpui::Window, cx: &mut gpui::App| {
            let kept = choices[i];
            match clipboard::set_keep_days(kept) {
                Ok(()) => {
                    window
                        .update(cx, |view, cx| {
                            view.keep_days = kept;
                            cx.notify();
                        })
                        .log_err();
                }
                Err(err) => eprintln!("topsy: could not change how long history is kept: {err}"),
            }
        };
        let keep = row(
            "keep",
            "Keep history for",
            "Pinned entries are always kept",
            segmented("days", choices.map(days), chosen, pick),
        );
        let clear = row(
            "clear",
            "Clear history",
            "Remove everything you haven't pinned",
            button("clear-button", "Clear", false)
                .on_click(|_, _, _| clipboard::send(clipboard::Command::Clear)),
        );
        [keep.into_any_element(), clear.into_any_element()]
    }

    fn about(&self) -> Div {
        let badge = div()
            .size(px(64.0))
            .flex()
            .justify_center()
            .rounded(px(16.0))
            .bg(rgb(0x141417))
            .border_1()
            .border_color(rgba(0xffffff26))
            .child(
                div()
                    .mt(px(14.0))
                    .w(px(34.0))
                    .h(px(15.0))
                    .rounded_b(px(9.0))
                    .bg(rgba(0xffffffe6)),
            );
        let header = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.0))
            .pt(px(8.0))
            .child(badge)
            .child(
                div()
                    .pt(px(8.0))
                    .text_size(px(20.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("topsy"),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(rgba(DIM))
                    .child(format!("Version {}", update::VERSION)),
            );
        let ready = update::ready().map(|version| {
            row(
                "ready",
                format!("Update ready: {}", version.short()),
                "Downloaded, and installed when topsy restarts",
                button("restart", "Restart to update", true)
                    .on_click(|_, _, cx| update::restart(cx)),
            )
            .into_any_element()
        });
        let link = |id: &'static str, label: &'static str, line, url| {
            row(id, label, line, icons::icon(Icon::Open, 16.0, rgb(TEXT)))
                .on_click(move |_, _, _| open_link(url))
                .into_any_element()
        };
        let rows = (ready.into_iter())
            .chain([
                link(
                    "repo",
                    "Source code",
                    "Issues, releases and the code, on GitHub",
                    REPO,
                ),
                link("docs", "Help", "What each part of topsy does", DOCS),
            ])
            .collect();
        div()
            .flex()
            .flex_col()
            .child(header)
            .child(section(None, rows))
    }
}
