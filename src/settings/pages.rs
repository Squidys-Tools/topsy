//! The settings window's pages and what each one holds. Every [`Setting`] belongs to a group, and
//! each group to a page, so a new setting is a compile error here until it has a place.

use std::path::Path;

use gpui::{AnyElement, Context, Div, SharedString, div, prelude::*, px, rgb, rgba};

use super::controls::{DIM, HOVER, TEXT, button, card, row, segmented, switch, title};
use super::window::SettingsWindow;
use crate::icons::{self, Icon};
use crate::log::LogErr;
use crate::settings::Setting;
use crate::{clipboard, update, win32};

const REPO: &str = "https://github.com/Squidys-Tools/topsy";
const DOCS: &str = "https://github.com/Squidys-Tools/topsy/tree/main/docs/user";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    General,
    Notch,
    Shelf,
    Clipboard,
    About,
}

impl Page {
    /// In the order the sidebar lists them.
    pub(super) const ALL: [Page; 5] = [
        Page::General,
        Page::Notch,
        Page::Shelf,
        Page::Clipboard,
        Page::About,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Notch => "Notch",
            Page::Shelf => "Widgets",
            Page::Clipboard => "Clipboard",
            Page::About => "About",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Startup,
    Look,
    Shows,
    Clipboard,
    Updates,
}

fn group(setting: Setting) -> Group {
    match setting {
        Setting::Autostart | Setting::SendTo => Group::Startup,
        Setting::Black | Setting::LiveGlass => Group::Look,
        Setting::VolumeHud | Setting::LiveActivities => Group::Shows,
        Setting::Clipboard => Group::Clipboard,
        Setting::Updates => Group::Updates,
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
        let line: SharedString = match page {
            Page::General => "How topsy starts, and where it shows up in Windows.".into(),
            Page::Notch => "How the island looks, and what it shows besides the shelf.".into(),
            Page::Shelf => {
                "Cards across the top of the tray's Home, left to right. Drag to reorder.".into()
            }
            Page::Clipboard => "What topsy keeps of what you copy.".into(),
            Page::About => format!("Version {}", update::VERSION).into(),
        };
        let body = div().flex().flex_col().child(title(page.name(), line));
        match page {
            Page::General => body.child(self.group(Group::Startup, None, cx)),
            Page::Notch => body
                .child(self.group(Group::Look, Some("Look"), cx))
                .child(self.group(Group::Shows, Some("Shows"), cx)),
            Page::Shelf => body.children(self.lineup_cards(cx)),
            Page::Clipboard => body.child(self.group(Group::Clipboard, None, cx)),
            Page::About => body.children(self.about(cx)),
        }
    }

    /// A card of `which`'s settings, a switch each, and what else belongs with them.
    fn group(&self, which: Group, heading: Option<&'static str>, cx: &mut Context<Self>) -> Div {
        let mut rows: Vec<AnyElement> = Setting::ALL
            .into_iter()
            .enumerate()
            .filter(|(_, setting)| group(*setting) == which)
            .map(|(i, setting)| {
                row(
                    ("setting", i),
                    setting.label(),
                    Some(setting.description()),
                    switch(self.on[i]),
                )
                .hover(|row| row.bg(rgba(HOVER)))
                .on_click(cx.listener(move |view, _, _, cx| view.toggle(i, setting, cx)))
                .into_any_element()
            })
            .collect();
        if which == Group::Clipboard {
            rows.extend(self.retention(cx));
        }
        card(heading, rows)
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
            Some("Pinned entries are always kept"),
            segmented("days", choices.map(days), chosen, pick),
        );
        let clear = row(
            "clear",
            "Clear history",
            Some("Remove everything you haven't pinned"),
            button("clear-button", "Clear", false)
                .on_click(|_, _, _| clipboard::send(clipboard::Command::Clear)),
        );
        [keep.into_any_element(), clear.into_any_element()]
    }

    fn about(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let ready = update::ready().map(|version| {
            card(
                None,
                vec![
                    row(
                        "ready",
                        format!("Update ready: {}", version.short()),
                        Some("Downloaded, and installed when topsy restarts"),
                        button("restart", "Restart to update", true)
                            .on_click(|_, _, cx| update::restart(cx)),
                    )
                    .into_any_element(),
                ],
            )
        });
        let link = |id: &'static str, label: &'static str, line, url| {
            row(
                id,
                label,
                Some(line),
                icons::icon(Icon::Open, 16.0, rgba(DIM)),
            )
            .hover(|row| row.bg(rgba(HOVER)))
            .on_click(move |_, _, _| open_link(url))
            .into_any_element()
        };
        let links = card(
            Some("Links"),
            vec![
                link(
                    "repo",
                    "Source code",
                    "Issues, releases and the code, on GitHub",
                    REPO,
                ),
                link(
                    "docs",
                    "Help",
                    "What each part of topsy does and how to use it",
                    DOCS,
                ),
            ],
        );
        let quit = card(
            None,
            vec![
                row(
                    "quit",
                    "Quit topsy",
                    Some("Stop topsy and remove its icon until you start it again"),
                    button("quit-button", "Quit", false).on_click(|_, _, cx| cx.quit()),
                )
                .into_any_element(),
            ],
        );
        ready
            .into_iter()
            .chain([self.group(Group::Updates, Some("Updates"), cx), links, quit])
            .collect()
    }
}

/// The notch's tab for `page`, raised when it is the one shown; `dot` marks news on it.
pub(super) fn tab(page: Page, current: bool, dot: bool) -> gpui::Stateful<Div> {
    div()
        .id(SharedString::from(page.name()))
        .flex()
        .items_center()
        .gap(px(5.0))
        .px(px(12.0))
        .py(px(5.0))
        .rounded_full()
        .text_size(px(13.0))
        .when(current, |tab| tab.bg(rgba(0xffffff24)))
        .when(!current, |tab| {
            tab.text_color(rgba(DIM))
                .hover(|tab| tab.text_color(rgb(TEXT)))
        })
        .child(page.name())
        .children(dot.then(|| div().size(px(5.0)).rounded_full().bg(rgb(TEXT))))
}
