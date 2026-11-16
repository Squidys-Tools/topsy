//! The clipboard strip: Ctrl+Alt+V opens a black strip of cards along the bottom of the screen
//! the cursor is on, newest copy first. Unlike the island it takes the focus, since it is typed
//! into: arrows choose, Enter or a click pastes into the app that was in front, typing
//! searches. It closes when it loses the focus. There is at most one.

use std::sync::Arc;

use gpui::{
    App, Bounds, ClickEvent, Context, FocusHandle, Global, KeyDownEvent, MouseButton, MouseUpEvent,
    ScrollHandle, Subscription, TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, div, point, prelude::*, px, rgb, rgba, size,
};
use raw_window_handle::HasWindowHandle;
use windows::Win32::Foundation::HWND;

use super::card::{self, card};
use super::pictures::Pictures;
use super::{Command, Event};
use crate::icons::{Icon, icon};
use crate::log;
use crate::model::clip_kind::{Kind, matches};
use crate::model::clipboard::Entry;
use crate::win32::{self, MenuEntry, paste};

/// The strip's height and its gap to the screen's edges, in logical pixels.
const HEIGHT: f32 = 284.0;
const MARGIN: f32 = 12.0;
const BACKGROUND: u32 = 0x0b0b0c;
/// A card per copy would be thousands of cards; older ones are a search away.
const MAX_SHOWN: usize = 100;

struct Open(WindowHandle<Strip>);

impl Global for Open {}

pub struct Strip {
    focus: FocusHandle,
    scroll: ScrollHandle,
    entries: Arc<[Entry]>,
    /// Each entry's kind, worked out once per change to the history.
    kinds: Vec<Kind>,
    /// Indexes into `entries` that the search and tab let through, at most `MAX_SHOWN`.
    shown: Vec<usize>,
    selected: usize,
    query: String,
    pinned_only: bool,
    /// The window in front when the hotkey was pressed, which a paste goes back to.
    previous: usize,
    scale: f32,
    pictures: Pictures,
    /// A card's menu is up; losing the focus to it doesn't close the strip.
    menu: bool,
    _activation: Option<Subscription>,
}

/// Act on what the clipboard's threads report, on the UI thread.
pub fn listen(cx: &mut App, events: async_channel::Receiver<Event>) {
    cx.spawn(async move |cx| {
        while let Ok(event) = events.recv().await {
            let _ = cx.update(|cx| match event {
                Event::Hotkey(previous) => toggle(cx, previous),
                Event::Changed => refresh(cx),
            });
        }
    })
    .detach();
}

fn hwnd(window: &Window) -> Option<HWND> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    win32::hwnd_from_raw(handle.as_raw())
}

fn toggle(cx: &mut App, previous: usize) {
    let open = cx.try_global::<Open>().map(|open| open.0);
    if let Some(handle) = open
        && handle
            .update(cx, |strip, window, _| strip.close(window, true))
            .is_ok()
    {
        return;
    }
    log("clipboard strip");
    let Some((_, work, scale)) = win32::cursor_monitor() else {
        return;
    };
    let margin = (MARGIN * scale).round() as i32;
    let (x, w) = (work.left + margin, work.right - work.left - 2 * margin);
    let h = (HEIGHT * scale).round() as i32;
    let y = work.bottom - margin - h;
    let logical = |v: i32| px(v as f32 / scale);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(logical(x), logical(y)),
            size(logical(w), logical(h)),
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("topsy clipboard".into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Opaque,
        window_min_size: Some(size(px(1.0), px(1.0))),
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        let hwnd = hwnd(window);
        if let Some(hwnd) = hwnd {
            win32::apply_strip_styles(hwnd);
            win32::round_corners(hwnd);
            win32::bring_to_front(hwnd);
            // GPUI placed it by the primary display's scale; this one's may differ.
            let raw = hwnd.0 as usize;
            cx.defer(move |_| win32::move_window(HWND(raw as *mut _), x, y, w, h, true));
        }
        cx.new(|cx| {
            let focus = cx.focus_handle();
            window.focus(&focus);
            let activation = cx.observe_window_activation(
                window,
                |strip: &mut Strip, window: &mut Window, _| {
                    if !window.is_window_active() && !strip.menu {
                        strip.close(window, false);
                    }
                },
            );
            let mut strip = Strip {
                focus,
                scroll: ScrollHandle::new(),
                entries: Arc::new([]),
                kinds: Vec::new(),
                shown: Vec::new(),
                selected: 0,
                query: String::new(),
                pinned_only: false,
                previous,
                scale,
                pictures: Pictures::default(),
                menu: false,
                _activation: Some(activation),
            };
            strip.reload(cx);
            strip
        })
    });
    match opened {
        Ok(handle) => cx.set_global(Open(handle)),
        Err(err) => eprintln!("topsy: could not open the clipboard strip: {err}"),
    }
}

fn refresh(cx: &mut App) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0) {
        let _ = handle.update(cx, |strip, _, cx| strip.reload(cx));
    }
}

impl Strip {
    fn reload(&mut self, cx: &mut Context<Self>) {
        // A copy or an expiry while open shifts the list; stay on the same entry.
        let held = self.entry(self.selected).map(|e| e.id);
        self.entries = super::entries();
        self.kinds = self.entries.iter().map(|e| e.content.kind()).collect();
        self.filter(cx);
        if let Some(at) = held.and_then(|id| {
            (0..self.shown.len()).find(|&i| self.entry(i).is_some_and(|e| e.id == id))
        }) {
            self.select(at);
        }
    }

    fn filter(&mut self, cx: &mut Context<Self>) {
        self.shown = (self.entries.iter().zip(&self.kinds).enumerate())
            .filter(|&(_, (e, &kind))| {
                (!self.pinned_only || e.pinned) && matches(e, kind, &self.query)
            })
            .map(|(i, _)| i)
            .take(MAX_SHOWN)
            .collect();
        self.select(self.selected);
        self.fetch(cx);
        cx.notify();
    }

    fn select(&mut self, at: usize) {
        self.selected = at.min(self.shown.len().saturating_sub(1));
        self.scroll.scroll_to_item(self.selected);
    }

    fn entry(&self, at: usize) -> Option<&Entry> {
        self.entries.get(*self.shown.get(at)?)
    }

    fn fetch(&mut self, cx: &mut Context<Self>) {
        let wants = (self.shown.iter()).flat_map(|&i| card::wants(&self.entries[i], self.kinds[i]));
        let wants: Vec<_> = wants.collect();
        self.pictures
            .fetch(wants, self.scale, cx, |strip: &mut Self| {
                &mut strip.pictures
            });
    }

    /// Close the strip; `give_back` returns the focus to the app it came from.
    fn close(&mut self, window: &mut Window, give_back: bool) {
        self._activation = None;
        if give_back && hwnd(window) == Some(win32::foreground_window()) {
            win32::restore_foreground(HWND(self.previous as *mut _));
        }
        window.remove_window();
    }

    /// Put the entry back on the clipboard and, with `paste`, paste it into the app that was
    /// in front. The strip closes first, so that app gets the focus back.
    fn take(&mut self, at: usize, paste: bool, window: &mut Window) {
        let Some(entry) = self.entry(at).cloned() else {
            return;
        };
        let previous = self.previous;
        self.close(window, false);
        log(&format!("clipboard take {} paste {paste}", entry.id));
        std::thread::spawn(move || {
            let put = super::data(&entry).is_some_and(|data| paste::put(&data));
            let previous = HWND(previous as *mut _);
            if put && paste {
                if !paste::paste_into(previous) {
                    log("clipboard paste skipped: no keys sent");
                }
            } else {
                if !put {
                    eprintln!("topsy: could not put clipboard entry {} back", entry.id);
                }
                win32::restore_foreground(previous);
            }
        });
    }

    fn pin(&self, at: usize) {
        if let Some(entry) = self.entry(at) {
            super::send(Command::Pin(entry.id, !entry.pinned));
        }
    }

    fn remove(&self, at: usize) {
        if let Some(entry) = self.entry(at) {
            super::send(Command::Remove(entry.id));
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let ctrl = key.modifiers.control;
        let command = ctrl || key.modifiers.alt || key.modifiers.platform;
        match key.key.as_str() {
            "escape" if !self.query.is_empty() => self.query.clear(),
            "escape" => return self.close(window, true),
            "enter" => return self.take(self.selected, true, window),
            "c" if ctrl => return self.take(self.selected, false, window),
            "p" if ctrl => return self.pin(self.selected),
            "delete" => return self.remove(self.selected),
            "left" => self.select(self.selected.saturating_sub(1)),
            "right" => self.select(self.selected + 1),
            "home" => self.select(0),
            "end" => self.select(usize::MAX),
            "tab" => self.pinned_only = !self.pinned_only,
            "backspace" => {
                self.query.pop();
            }
            _ => match key.key_char.as_deref() {
                Some(typed) if !command && !typed.chars().any(char::is_control) => {
                    self.query.push_str(typed);
                }
                _ => return,
            },
        }
        cx.stop_propagation();
        if !matches!(key.key.as_str(), "left" | "right" | "home" | "end") {
            self.selected = 0;
            self.filter(cx);
        }
        cx.notify();
    }

    /// A card's menu, at the cursor. It runs its own message loop, so on a thread of its own
    /// (see [`win32::popup_menu`]); the choice comes back here.
    fn menu(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, pinned)) = self.entry(at).map(|e| (e.id, e.pinned)) else {
            return;
        };
        self.select(at);
        cx.notify();
        let entries = [
            MenuEntry::Item(1, "&Paste"),
            MenuEntry::Item(2, "&Copy"),
            MenuEntry::Item(3, if pinned { "Unp&in" } else { "P&in" }),
            MenuEntry::Separator,
            MenuEntry::Item(4, "&Delete"),
        ];
        let (tx, rx) = async_channel::bounded(1);
        let spawned = std::thread::Builder::new()
            .name("topsy-clip-menu".into())
            .spawn(move || {
                let _ = tx.send_blocking(win32::popup_menu(&entries));
            });
        if spawned.is_err() {
            return;
        }
        self.menu = true;
        cx.spawn_in(window, async move |this, cx| {
            let choice = rx.recv().await.ok().flatten();
            let _ = this.update_in(cx, |this, window, _| {
                this.menu = false;
                // The history may have moved on while the menu was up.
                let Some(at) =
                    (0..this.shown.len()).find(|&at| this.entry(at).map(|e| e.id) == Some(id))
                else {
                    return;
                };
                match choice {
                    Some(1) => this.take(at, true, window),
                    Some(2) => this.take(at, false, window),
                    Some(3) => this.pin(at),
                    Some(4) => this.remove(at),
                    _ => {}
                }
            });
        })
        .detach();
    }

    fn tab(&self, label: &'static str, pinned: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.pinned_only == pinned;
        div()
            .id(label)
            .px(px(12.0))
            .py(px(4.0))
            .rounded_full()
            .when(active, |tab| {
                tab.bg(rgb(0xf2f2f2)).text_color(rgb(0x1c1c1e))
            })
            .when(!active, |tab| tab.hover(|tab| tab.bg(rgba(0xffffff14))))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.pinned_only = pinned;
                this.selected = 0;
                this.filter(cx);
            }))
    }
}

impl Render for Strip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = super::now();
        let search = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .min_w(px(180.0))
            .px(px(12.0))
            .py(px(4.0))
            .rounded_full()
            .bg(rgba(0xffffff14))
            .child(icon(Icon::Search, 14.0, rgba(0xffffff8c)))
            .child(if self.query.is_empty() {
                div().text_color(rgba(0xffffff66)).child("Type to search")
            } else {
                div().child(format!("{}\u{258f}", self.query))
            });
        let hint = div().text_xs().text_color(rgba(0xffffff66)).child(
            "Enter paste \u{b7} Ctrl+C copy \u{b7} Ctrl+P pin \u{b7} Del delete \u{b7} Tab pinned",
        );
        let bar = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(4.0))
            .child(search)
            .child(self.tab("Clipboard", false, cx))
            .child(self.tab("Pinned", true, cx))
            .child(div().flex_1())
            .child(hint);
        let cards = self.shown.iter().enumerate().map(|(at, &i)| {
            let entry = &self.entries[i];
            let selected = at == self.selected;
            let more = div()
                .id(("more", entry.id as usize))
                .px(px(6.0))
                .rounded_full()
                .py(px(2.0))
                .hover(|more| more.bg(rgba(0xffffff26)))
                .child(icon(Icon::More, 16.0, rgba(0xffffffb3)))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    this.menu(at, window, cx);
                }))
                .into_any_element();
            card(entry, self.kinds[i], selected, now, &self.pictures, more)
                .id(("card", entry.id as usize))
                .when(!selected, |card| {
                    card.hover(|card| card.border_color(rgba(0xffffff59)))
                })
                .on_click(cx.listener(move |this, _, window, _| this.take(at, true, window)))
                .on_mouse_up(
                    MouseButton::Right,
                    cx.listener(move |this, _: &MouseUpEvent, window, cx| {
                        this.menu(at, window, cx)
                    }),
                )
        });
        let row = if self.shown.is_empty() {
            let empty = if self.entries.is_empty() {
                "Nothing copied yet"
            } else {
                "Nothing matches"
            };
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgba(0xffffff66))
                .child(empty)
                .into_any_element()
        } else {
            div()
                .id("cards")
                .flex()
                .gap(px(12.0))
                .overflow_x_scroll()
                .track_scroll(&self.scroll)
                .children(cards)
                .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .p(px(12.0))
            .bg(rgb(BACKGROUND))
            .text_color(rgb(0xffffff))
            .text_sm()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .child(bar)
            .child(row)
    }
}
