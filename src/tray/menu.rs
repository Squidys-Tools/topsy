//! The tray icon's menu, laid out like Droppy's menu-bar menu, in the notch's glass: a header
//! with the version, the shelf, basket and clipboard, the settings in an *Options* submenu,
//! then the update and Quit. Unlike the notch it takes the focus while it is up, as a menu
//! does, for the arrows, Enter, Escape and the access keys; it closes when it loses it. The
//! submenu is a second window that never activates, so the menu keeps the keyboard.
//! Every choice goes to the island as the [`TrayEvent`] the old Win32 menu sent. What it lists
//! is in `lines`.

use gpui::{
    AnyWindowHandle, App, Context, FocusHandle, Global, KeyDownEvent, Subscription, WeakEntity,
    Window, WindowHandle,
};
use windows::Win32::Foundation::HWND;

use super::TrayEvent;
use super::lines::{Item, Line, PAD, ROW, SUB_WIDTH, WIDTH, height, lines, top_of};
use crate::frame::{Frame, Popup, Shape, open_window};
use crate::settings::Setting;
use crate::{clipboard, log, win32};
use model::menu;

/// What the menu shows, read once as it opens.
#[derive(Clone, Copy)]
pub(super) struct Counts {
    pub shelf: usize,
    pub basket: usize,
    pub clipboard: bool,
    /// Whether the floating player is out; none without Now Playing.
    pub player: Option<bool>,
}

/// What foreground does as the menu closes.
#[derive(Clone, Copy)]
enum Hand {
    /// Back to the window that had it, for a choice that opens nothing.
    Back,
    /// Kept by topsy, for a window of its own that is about to come to the front.
    Keep,
    /// Left alone: the user clicked somewhere else.
    Leave,
}

struct Open(WindowHandle<Menu>);

impl Global for Open {}

pub struct Menu {
    pub(super) lines: Vec<Line>,
    pub(super) counts: Counts,
    pub(super) on: [bool; Setting::ALL.len()],
    /// The line under the pointer or the arrows, in the menu and in the submenu.
    pub(super) highlight: Option<usize>,
    pub(super) sub_highlight: Option<usize>,
    /// The arrows are in the submenu.
    in_sub: bool,
    sub: Option<WindowHandle<Submenu>>,
    pub(super) frame: Frame,
    pub(super) focus: FocusHandle,
    hwnd: Option<HWND>,
    handle: Option<AnyWindowHandle>,
    /// The window in front before the menu, which gets the foreground back.
    previous: usize,
    /// Where it opened and on a monitor of what scale, for the submenu beside it.
    scale: f32,
    closed: bool,
    _activation: Option<Subscription>,
}

/// The submenu's window: the settings, drawn from the menu's state.
pub struct Submenu {
    pub(super) menu: WeakEntity<Menu>,
    pub(super) frame: Frame,
}

/// Open the menu at `at` (physical screen pixels), in place of one already up.
pub fn open(cx: &mut App, at: (i32, i32), previous: usize) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0) {
        handle
            .update(cx, |menu, _, cx| menu.close(Hand::Leave, cx))
            .ok();
    }
    let Some((work, scale)) = win32::monitor_at(at) else {
        return;
    };
    let counts = Counts {
        shelf: super::shelf_count(),
        basket: super::basket_count(),
        clipboard: clipboard::is_enabled(),
        player: super::player(),
    };
    let lines = lines(counts.player);
    let size_px = (WIDTH, height(&lines));
    let to_px = |v: f32| (v * scale).round() as i32;
    let origin = menu::place(at, (to_px(size_px.0), to_px(size_px.1)), work);
    log(&format!("menu at {origin:?}"));
    let opened = open_window(
        cx,
        Popup {
            title: "topsy menu",
            origin,
            size: size_px,
            scale,
            activate: true,
            shape: Shape::Floating,
        },
        move |hwnd, frame, window, cx| {
            if let Some(hwnd) = hwnd {
                win32::bring_to_front(hwnd);
            }
            let focus = cx.focus_handle();
            window.focus(&focus);
            let activation = cx.observe_window_activation(window, |menu: &mut Menu, window, cx| {
                if !window.is_window_active() {
                    menu.deactivated(cx);
                }
            });
            Menu {
                lines,
                counts,
                on: Setting::ALL.map(Setting::is_on),
                highlight: None,
                sub_highlight: None,
                in_sub: false,
                sub: None,
                frame,
                focus,
                hwnd,
                handle: None,
                previous,
                scale,
                closed: false,
                _activation: Some(activation),
            }
        },
    );
    match opened {
        Ok(handle) => {
            handle
                .update(cx, |menu, _, _| menu.handle = Some(handle.into()))
                .ok();
            cx.set_global(Open(handle));
        }
        Err(err) => {
            eprintln!("topsy: could not open the menu: {err}");
            win32::restore_foreground(HWND(previous as *mut _));
        }
    }
}

impl Menu {
    pub(super) fn item(&self, at: usize) -> Option<Item> {
        match self.lines.get(at)? {
            Line::Item(item) => Some(*item),
            Line::Header | Line::Gap => None,
        }
    }

    pub(super) fn enabled(&self, item: Item) -> bool {
        match item {
            Item::Clear => self.counts.shelf > 0,
            Item::Basket => self.counts.basket > 0,
            Item::Clipboard => self.counts.clipboard,
            Item::Show
            | Item::Player(_)
            | Item::Options
            | Item::Settings
            | Item::AddHeadset
            | Item::Update(_)
            | Item::Quit => true,
        }
    }

    pub(super) fn sub_open(&self) -> bool {
        self.sub.is_some()
    }

    /// Losing the focus closes it, unless what took it is its own submenu opening (which hands
    /// it straight back). Checked once that has settled.
    fn deactivated(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            this.update(cx, |menu, cx| {
                let front = win32::foreground_window();
                if menu.hwnd != Some(front) {
                    menu.close(Hand::Leave, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn close(&mut self, hand: Hand, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.closed = true;
        self._activation = None;
        match hand {
            Hand::Back => win32::restore_foreground(HWND(self.previous as *mut _)),
            Hand::Keep => super::hold_foreground(),
            Hand::Leave => {}
        }
        let windows = [self.handle.take(), self.sub.take().map(Into::into)];
        cx.defer(move |cx| {
            for handle in windows.into_iter().flatten() {
                handle
                    .update(cx, |_, window, _| window.remove_window())
                    .ok();
            }
        });
    }

    pub(super) fn hover(&mut self, at: usize, hovered: bool, cx: &mut Context<Self>) {
        let Some(item) = self.item(at).filter(|item| self.enabled(*item)) else {
            return;
        };
        if hovered {
            self.highlight = Some(at);
            self.in_sub = false;
            if item == Item::Options {
                self.open_sub(cx);
            } else {
                self.close_sub(cx);
            }
        } else if self.highlight == Some(at) && !(item == Item::Options && self.sub_open()) {
            self.highlight = None;
        }
        cx.notify();
    }

    pub(super) fn hover_sub(&mut self, at: usize, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self.sub_highlight = Some(at);
            self.in_sub = true;
        } else if self.sub_highlight == Some(at) {
            self.sub_highlight = None;
        }
        cx.notify();
    }

    fn open_sub(&mut self, cx: &mut Context<Self>) {
        if self.sub.is_some() {
            return;
        }
        let (Some(hwnd), Some(at)) = (
            self.hwnd,
            self.lines
                .iter()
                .position(|l| *l == Line::Item(Item::Options)),
        ) else {
            return;
        };
        let Some(parent) = win32::window_rect(hwnd) else {
            return;
        };
        let scale = self.scale;
        let to_px = |v: f32| (v * scale).round() as i32;
        let Some((work, _)) = win32::monitor_at((parent.left, parent.top)) else {
            return;
        };
        let size_px = (SUB_WIDTH, PAD * 2.0 + ROW * Setting::ALL.len() as f32);
        let origin = menu::beside(
            parent,
            parent.top + to_px(top_of(&self.lines, at)),
            to_px(PAD),
            (to_px(size_px.0), to_px(size_px.1)),
            work,
        );
        let menu = cx.entity().downgrade();
        let this = cx.entity().downgrade();
        // A window is made out of any update, and it takes the foreground as it is made, which
        // goes straight back to the menu.
        cx.defer(move |cx| {
            let opened = open_window(
                cx,
                Popup {
                    title: "topsy submenu",
                    origin,
                    size: size_px,
                    scale,
                    activate: false,
                    shape: Shape::Floating,
                },
                move |_, frame, _, cx| {
                    if let Some(menu) = menu.upgrade() {
                        cx.observe(&menu, |_, _, cx| cx.notify()).detach();
                    }
                    win32::bring_to_front(hwnd);
                    Submenu { menu, frame }
                },
            );
            match opened {
                Ok(handle) => {
                    let kept = this.update(cx, |menu, _| {
                        if menu.closed
                            || menu.sub.is_some()
                            || menu.highlight_item() != Some(Item::Options)
                        {
                            false
                        } else {
                            menu.sub = Some(handle);
                            true
                        }
                    });
                    if !kept.unwrap_or(false) {
                        handle
                            .update(cx, |_, window, _| window.remove_window())
                            .ok();
                    }
                }
                Err(err) => eprintln!("topsy: could not open the submenu: {err}"),
            }
        });
    }

    fn close_sub(&mut self, cx: &mut Context<Self>) {
        self.in_sub = false;
        self.sub_highlight = None;
        if let Some(handle) = self.sub.take() {
            // Out of this update: the submenu may be the window handling the event.
            cx.defer(move |cx| {
                handle
                    .update(cx, |_, window, _| window.remove_window())
                    .ok();
            });
        }
    }

    fn highlight_item(&self) -> Option<Item> {
        self.highlight.and_then(|at| self.item(at))
    }

    pub(super) fn choose(&mut self, item: Item, cx: &mut Context<Self>) {
        if !self.enabled(item) {
            return;
        }
        let previous = self.previous;
        let (hand, event) = match item {
            Item::Show => (Hand::Back, TrayEvent::Show),
            Item::Clear => (Hand::Back, TrayEvent::ClearShelf),
            Item::Basket => (Hand::Back, TrayEvent::ShowBasket),
            Item::Player(_) => (Hand::Back, TrayEvent::Player),
            Item::Clipboard => (Hand::Keep, TrayEvent::Clipboard(previous)),
            Item::Options => {
                self.highlight = self.lines.iter().position(|l| *l == Line::Item(item));
                self.in_sub = true;
                self.sub_highlight = Some(0);
                self.open_sub(cx);
                cx.notify();
                return;
            }
            Item::Settings => (Hand::Back, TrayEvent::OpenSettings),
            Item::AddHeadset => (Hand::Leave, TrayEvent::AddHeadset),
            Item::Update(_) => (Hand::Back, TrayEvent::Update),
            Item::Quit => (Hand::Back, TrayEvent::Quit),
        };
        self.close(hand, cx);
        super::send(event);
    }

    pub(super) fn choose_setting(&mut self, setting: Setting, cx: &mut Context<Self>) {
        self.close(Hand::Back, cx);
        if let Some(on) = setting.toggle() {
            super::send(TrayEvent::Setting(setting, on));
        }
    }

    pub(super) fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        match key.key.as_str() {
            "escape" | "left" if self.in_sub => self.close_sub(cx),
            "escape" => return self.close(Hand::Back, cx),
            "down" | "up" if self.in_sub => {
                let all = [true; Setting::ALL.len()];
                self.sub_highlight = menu::step(&all, self.sub_highlight, key.key == "down");
            }
            "down" | "up" => {
                let choosable: Vec<bool> = (0..self.lines.len())
                    .map(|at| self.item(at).is_some_and(|item| self.enabled(item)))
                    .collect();
                self.highlight = menu::step(&choosable, self.highlight, key.key == "down");
                self.close_sub(cx);
            }
            "right" if !self.in_sub && self.highlight_item() == Some(Item::Options) => {
                return self.choose(Item::Options, cx);
            }
            "enter" | "space" if self.in_sub => {
                if let Some(setting) = self.sub_highlight.and_then(|at| Setting::ALL.get(at)) {
                    return self.choose_setting(*setting, cx);
                }
            }
            "enter" | "space" => {
                if let Some(item) = self.highlight_item() {
                    return self.choose(item, cx);
                }
            }
            _ => {
                let Some(typed) = key.key_char.as_deref().and_then(|t| t.chars().next()) else {
                    return;
                };
                if self.in_sub {
                    let labels = Setting::ALL.map(Setting::menu_label);
                    if let Some(at) = menu::by_access_key(labels, typed) {
                        return self.choose_setting(Setting::ALL[at], cx);
                    }
                } else {
                    let items: Vec<Item> = (0..self.lines.len())
                        .filter_map(|at| self.item(at))
                        .collect();
                    let labels = items.iter().map(|item| item.label());
                    if let Some(at) = menu::by_access_key(labels, typed) {
                        return self.choose(items[at], cx);
                    }
                }
                return;
            }
        }
        cx.stop_propagation();
        cx.notify();
    }
}
