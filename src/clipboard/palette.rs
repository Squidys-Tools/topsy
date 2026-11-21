//! The clipboard palette: Ctrl+Alt+V drops a launcher out of the top edge of the screen the
//! cursor is on, a notch of glass of its own that springs open as the tray does: a search over
//! a list of what was copied (pinned first, then by day) beside a preview of the selected copy.
//! Unlike the island it takes the focus, since it is typed into: arrows choose, Enter or a
//! double-click pastes into the app that was in front, typing searches. It closes when it loses
//! the focus. There is at most one.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use gpui::{
    App, Context, FocusHandle, Global, KeyDownEvent, ScrollHandle, Subscription, Window,
    WindowHandle,
};
use windows::Win32::Foundation::HWND;

use super::actions::Panel;
use super::pictures::Pictures;
use super::view::{self, HEIGHT, WIDTH};
use super::{Command, Event, detail};
use crate::frame::{self, Frame, Popup, Shape, open_window};
use crate::log;
use crate::log::LogErr;
use crate::win32::{self, clock, paste};
use model::clip_kind::Kind;
use model::clip_list::{Section, listed, section};
use model::clipboard::Entry;
use model::spring::{Spring, SpringConfig};

/// A row per copy would be thousands of rows; older ones are a search away.
const MAX_SHOWN: usize = 100;

struct Open(WindowHandle<Palette>);

impl Global for Open {}

/// The palette's height, springing out of the top edge to its full height.
fn opening() -> Spring {
    let mut height = Spring::new(0.0, SpringConfig::ISLAND);
    height.set_target(HEIGHT);
    height
}

pub struct Palette {
    pub(super) focus: FocusHandle,
    pub(super) scroll: ScrollHandle,
    pub(super) frame: Frame,
    pub(super) height: Spring,
    last_frame: Option<Instant>,
    /// Folding back into the top edge, to close once it is in.
    folding: bool,
    pub(super) entries: Arc<[Entry]>,
    /// Each entry's kind, worked out once per change to the history.
    pub(super) kinds: Vec<Kind>,
    /// Indexes into `entries` that the search and the filter let through, at most `MAX_SHOWN`.
    pub(super) shown: Vec<usize>,
    pub(super) selected: usize,
    pub(super) query: String,
    pub(super) pinned_only: bool,
    /// When today began, local time, for the list's sections.
    midnight: u64,
    /// The window in front when the hotkey was pressed, which a paste goes back to, and its exe.
    previous: usize,
    pub(super) target: Option<PathBuf>,
    hwnd: Option<HWND>,
    scale: f32,
    pub(super) pictures: Pictures,
    /// The actions panel, over the list, while it is open.
    pub(super) panel: Option<Panel>,
    _activation: Option<Subscription>,
}

/// Act on what the clipboard's threads report, on the UI thread.
pub fn listen(cx: &mut App, events: async_channel::Receiver<Event>) {
    cx.spawn(async move |cx| {
        while let Ok(event) = events.recv().await {
            cx.update(|cx| match event {
                Event::Hotkey(previous) => toggle(cx, previous),
                Event::Changed => refresh(cx),
            })
            .log_err();
        }
    })
    .detach();
}

/// Open the palette from the tray icon's menu; one already open stays.
pub(crate) fn show(cx: &mut App, previous: usize) {
    let open = cx.try_global::<Open>().map(|open| open.0);
    if open.is_none_or(|handle| handle.update(cx, |_, _, _| ()).is_err()) {
        toggle(cx, previous);
    }
}

fn toggle(cx: &mut App, previous: usize) {
    let open = cx.try_global::<Open>().map(|open| open.0);
    if let Some(handle) = open
        && handle
            .update(cx, |palette, _, cx| palette.fold(cx))
            .unwrap_or(false)
    {
        return;
    }
    log("clipboard palette");
    let Some((_, work, scale)) = win32::cursor_monitor() else {
        return;
    };
    // Anywhere on the monitor's top edge: the frame hangs it from the middle.
    let origin = ((work.left + work.right) / 2, work.top);
    let target = win32::window_exe(HWND(previous as *mut _));
    let opened = open_window(
        cx,
        Popup {
            title: "topsy clipboard",
            origin,
            size: (WIDTH, HEIGHT),
            scale,
            activate: true,
            shape: Shape::Hanging,
        },
        move |hwnd, frame, window, cx| {
            if let Some(hwnd) = hwnd {
                win32::bring_to_front(hwnd);
                // It hangs where the island does: an island appearing now goes under it.
                frame::keep_on_top(hwnd);
            }
            let focus = cx.focus_handle();
            window.focus(&focus);
            let activation = cx.observe_window_activation(
                window,
                |palette: &mut Palette, window: &mut Window, _| {
                    if !window.is_window_active() {
                        palette.close(window);
                    }
                },
            );
            let mut palette = Palette {
                focus,
                scroll: ScrollHandle::new(),
                frame,
                height: opening(),
                last_frame: None,
                folding: false,
                entries: Arc::new([]),
                kinds: Vec::new(),
                shown: Vec::new(),
                selected: 0,
                query: String::new(),
                pinned_only: false,
                midnight: 0,
                previous,
                target,
                hwnd,
                scale,
                pictures: Pictures::default(),
                panel: None,
                _activation: Some(activation),
            };
            palette.reload(cx);
            palette
        },
    );
    match opened {
        Ok(handle) => cx.set_global(Open(handle)),
        Err(err) => eprintln!("topsy: could not open the clipboard palette: {err}"),
    }
}

fn refresh(cx: &mut App) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0) {
        handle
            .update(cx, |palette, _, cx| palette.reload(cx))
            .log_err();
    }
}

impl Palette {
    fn reload(&mut self, cx: &mut Context<Self>) {
        // A copy or an expiry while open shifts the list; stay on the same entry.
        let held = self.entry(self.selected).map(|e| e.id);
        let now = super::now();
        self.midnight = now.saturating_sub(clock::seconds_into_day());
        self.entries = super::entries();
        self.kinds = self.entries.iter().map(|e| e.content.kind()).collect();
        self.filter(cx);
        if let Some(at) = held.and_then(|id| self.position(id)) {
            self.select(at, cx);
        }
        // Its entry deleted or expired, the actions panel has nothing to act on.
        if let Some(panel) = &self.panel
            && self.position(panel.id).is_none()
        {
            self.panel = None;
        }
    }

    fn filter(&mut self, cx: &mut Context<Self>) {
        self.shown = listed(
            &self.entries,
            &self.kinds,
            &self.query,
            self.pinned_only,
            MAX_SHOWN,
        );
        self.select(self.selected, cx);
        let wants = (self.shown.iter()).flat_map(|&i| view::wants(&self.entries[i], self.kinds[i]));
        let target = (self.target.iter()).flat_map(|exe| view::target_wants(exe));
        let wants: Vec<_> = wants.chain(target).collect();
        self.fetch(wants, cx);
        cx.notify();
    }

    fn fetch(&mut self, wants: Vec<super::pictures::Want>, cx: &mut Context<Self>) {
        self.pictures
            .fetch(wants, self.scale, cx, |palette: &mut Self| {
                &mut palette.pictures
            });
    }

    /// Select row `at` (the last, past the end), bring it into view and ask for its preview.
    pub(super) fn select(&mut self, at: usize, cx: &mut Context<Self>) {
        self.selected = at.min(self.shown.len().saturating_sub(1));
        self.scroll.scroll_to_item(self.child_of(self.selected));
        if let Some(entry) = self.entry(self.selected) {
            let wants = detail::wants(entry, self.kinds[self.shown[self.selected]]);
            self.fetch(wants, cx);
        }
        cx.notify();
    }

    pub(super) fn entry(&self, at: usize) -> Option<&Entry> {
        self.entries.get(*self.shown.get(at)?)
    }

    pub(super) fn position(&self, id: u64) -> Option<usize> {
        (0..self.shown.len()).find(|&at| self.entry(at).is_some_and(|e| e.id == id))
    }

    pub(super) fn section_of(&self, at: usize) -> Option<Section> {
        self.entry(at).map(|entry| section(entry, self.midnight))
    }

    /// Which child of the list row `at` is, counting the section headers above it; the first
    /// row of a section is brought into view with its header.
    fn child_of(&self, at: usize) -> usize {
        let mut headers = 0;
        let mut first = false;
        let mut last = None;
        for row in 0..=at.min(self.shown.len().saturating_sub(1)) {
            let section = self.section_of(row);
            first = section != last;
            if first {
                headers += 1;
                last = section;
            }
        }
        (at + headers).saturating_sub(usize::from(first))
    }

    /// Close the palette at once, leaving the focus where it is: a paste is about to hand it
    /// back, or something else took it.
    fn close(&mut self, window: &mut Window) {
        self._activation = None;
        if let Some(hwnd) = self.hwnd {
            frame::release_top(hwnd);
        }
        window.remove_window();
    }

    /// Give the focus back to the app the palette came from and fold it up into the top edge,
    /// as the tray folds; the window goes once it is folded. False if it was already folding.
    pub(super) fn fold(&mut self, cx: &mut Context<Self>) -> bool {
        if self.folding {
            return false;
        }
        self.folding = true;
        self._activation = None;
        if self.hwnd == Some(win32::foreground_window()) {
            win32::restore_foreground(HWND(self.previous as *mut _));
        }
        self.height.set_target(0.0);
        cx.notify();
        true
    }

    /// Step the spring for this frame: whether the palette is still moving.
    pub(super) fn step(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        let dt = (self.last_frame).map_or(1.0 / 120.0, |t| now.duration_since(t).as_secs_f32());
        let moving = self.height.step(dt);
        self.last_frame = moving.then_some(now);
        if moving {
            window.request_animation_frame();
        } else if self.folding {
            // Not from inside the draw: the window is gone once this update ends.
            cx.spawn_in(window, async move |this, cx| {
                this.update_in(cx, |this, window, _| this.close(window))
                    .log_err();
            })
            .detach();
        }
        moving
    }

    /// Put the entry back on the clipboard and, with `paste`, paste it into the app that was
    /// in front. The palette closes first, so that app gets the focus back.
    pub(super) fn take(&mut self, at: usize, paste: bool, window: &mut Window) {
        let Some(entry) = self.entry(at).cloned() else {
            return;
        };
        let previous = self.previous;
        self.close(window);
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

    pub(super) fn pin(&self, at: usize) {
        if let Some(entry) = self.entry(at) {
            super::send(Command::Pin(entry.id, !entry.pinned));
        }
    }

    pub(super) fn remove(&self, at: usize) {
        if let Some(entry) = self.entry(at) {
            super::send(Command::Remove(entry.id));
        }
    }

    /// A row was clicked: the first click selects it, a second pastes it.
    pub(super) fn click(
        &mut self,
        at: usize,
        clicks: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if clicks >= 2 {
            return self.take(at, true, window);
        }
        self.select(at, cx);
    }

    pub(super) fn set_pinned_only(&mut self, pinned_only: bool, cx: &mut Context<Self>) {
        self.pinned_only = pinned_only;
        self.selected = 0;
        self.filter(cx);
    }

    pub(super) fn on_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.folding {
            return;
        }
        if self.panel.is_some() {
            return self.on_panel_key(event, window, cx);
        }
        let key = &event.keystroke;
        let ctrl = key.modifiers.control;
        let command = ctrl || key.modifiers.alt || key.modifiers.platform;
        let mut refilter = true;
        match key.key.as_str() {
            "escape" if !self.query.is_empty() => self.query.clear(),
            "escape" => {
                self.fold(cx);
                return;
            }
            "enter" => return self.take(self.selected, !ctrl, window),
            "c" if ctrl => return self.take(self.selected, false, window),
            "p" if ctrl => return self.pin(self.selected),
            "k" if ctrl => return self.open_panel(self.selected, cx),
            "delete" => return self.remove(self.selected),
            "tab" => self.pinned_only = !self.pinned_only,
            "backspace" => {
                self.query.pop();
            }
            "up" | "down" | "home" | "end" | "pageup" | "pagedown" => {
                refilter = false;
                let page = 8;
                let at = match key.key.as_str() {
                    "up" => self.selected.saturating_sub(1),
                    "down" => self.selected + 1,
                    "home" => 0,
                    "pageup" => self.selected.saturating_sub(page),
                    "pagedown" => self.selected + page,
                    _ => usize::MAX,
                };
                self.select(at, cx);
            }
            // GPUI names the space key and gives it no typed character.
            "space" if !command => self.query.push(' '),
            _ => match key.key_char.as_deref() {
                Some(typed) if !command && !typed.chars().any(char::is_control) => {
                    self.query.push_str(typed);
                }
                _ => return,
            },
        }
        cx.stop_propagation();
        if refilter {
            self.selected = 0;
            self.filter(cx);
        }
    }
}
