//! The settings window, after Droppy's: a floating pane of the island's glass with its pages down
//! a sidebar, a switch for each [`Setting`] and the droplets' lineup among them. It
//! takes focus, since it is clicked through, and falls behind other windows once left. There is
//! at most one; asking for it again raises the one that is open.

use std::rc::Rc;

use gpui::{
    App, Context, FocusHandle, KeyDownEvent, Window, WindowControlArea, WindowHandle, div,
    prelude::*, px, rgb, rgba,
};
use windows::Win32::Foundation::HWND;

use super::controls::{DIM, LINE, TEXT, pill};
use super::pages::Page;
use crate::frame::{Frame, PANEL_RADIUS, Popup, Shape, open_window};
use crate::icons::{self, Icon};
use crate::log::LogErr;
use crate::settings::Setting;
use crate::{clipboard, droplets, update, win32};
use model::droplets::Lineup;

/// What the window changed, after it is saved.
pub enum Change {
    Setting(Setting, bool),
    Droplets(Lineup),
}

/// Told about every change the window makes, so the app can apply it.
pub type OnChange = Rc<dyn Fn(Change, &mut App)>;

const SIZE: (f32, f32) = (680.0, 480.0);
const SIDEBAR: f32 = 190.0;
/// The strip along the top that moves the window.
const CAPTION: f32 = 32.0;

struct Open(WindowHandle<SettingsWindow>);

impl gpui::Global for Open {}

pub struct SettingsWindow {
    page: Page,
    pub(super) on: [bool; Setting::ALL.len()],
    pub(super) keep_days: u32,
    pub(super) lineup: Lineup,
    pub(super) on_change: OnChange,
    frame: Frame,
    focus: FocusHandle,
    hwnd: Option<HWND>,
    /// The window in front before this one, given the foreground back when it closes.
    previous: usize,
}

impl SettingsWindow {
    fn reload(&mut self) {
        self.on = Setting::ALL.map(Setting::is_on);
        self.keep_days = clipboard::keep_days();
        self.lineup = droplets::saved();
    }

    /// Flip `setting`, the `i`th of [`Setting::ALL`], and tell the app.
    pub(super) fn toggle(&mut self, i: usize, setting: Setting, cx: &mut Context<Self>) {
        if let Some(on) = setting.toggle() {
            self.on[i] = on;
            if setting == Setting::Black {
                self.frame.set_black(on, cx);
            }
            cx.notify();
            (self.on_change.clone())(Change::Setting(setting, on), cx);
        }
    }

    fn close(&self, window: &mut Window) {
        if let Some(hwnd) = self.hwnd
            && win32::foreground_window() == hwnd
        {
            win32::restore_foreground(HWND(self.previous as *mut _));
        }
        window.remove_window();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, _: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            self.close(window);
        }
    }

    fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page != page {
            self.page = page;
            cx.notify();
        }
    }

    /// The pages down the left, and what is always at hand below them.
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let pages = Page::ALL.map(|page| {
            pill(
                page.name(),
                Some(page.icon()),
                page.name(),
                page == self.page,
            )
            .on_click(cx.listener(move |view, _, _, cx| view.go(page, cx)))
        });
        let restart = update::ready().map(|_| {
            pill("restart", Some(Icon::Refresh), "Restart to update", true)
                .on_click(|_, _, cx| update::restart(cx))
        });
        div()
            .flex_none()
            .w(px(SIDEBAR))
            .h_full()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .px(px(12.0))
            .pt(px(CAPTION + 8.0))
            .pb(px(12.0))
            .bg(rgba(0xffffff08))
            .border_r_1()
            .border_color(rgba(LINE))
            .children(pages)
            .child(div().flex_1())
            .children(restart)
            .child(
                pill("quit", None, "Quit topsy", false)
                    .justify_center()
                    .on_click(|_, _, cx| cx.quit()),
            )
    }

    /// The strip along the top that moves the window: its name on the left, close on the right.
    fn caption(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let name = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .pl(px(14.0))
            .text_size(px(12.0))
            .text_color(rgba(DIM))
            .child(
                div()
                    .w(px(16.0))
                    .h(px(9.0))
                    .rounded_b(px(6.0))
                    .bg(rgb(TEXT)),
            )
            .child("topsy settings");
        let close = div()
            .id("close")
            .occlude()
            .w(px(46.0))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .rounded_tr(px(PANEL_RADIUS))
            .hover(|button| button.bg(rgb(0xc42b1c)))
            .on_click(cx.listener(|view, _, window, _| view.close(window)))
            .child(icons::icon(Icon::Close, 12.0, rgb(TEXT)));
        div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(px(CAPTION))
            .flex()
            .justify_between()
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    // GPUI's `start_window_move` does nothing on Windows; a caption area lets
                    // Windows move it. Occluded, or the focus-on-click marks the press handled.
                    .window_control_area(WindowControlArea::Drag)
                    .occlude()
                    .child(name),
            )
            .child(close)
    }
}

/// Open the window, or bring the open one to the front.
pub fn open(cx: &mut App, on_change: OnChange, previous: usize) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0)
        && handle
            .update(cx, |view, _, cx| {
                view.reload();
                cx.notify();
                if let Some(hwnd) = view.hwnd {
                    win32::bring_to_front(hwnd);
                }
            })
            .is_ok()
    {
        return;
    }
    let Some((_, work, scale)) = win32::cursor_monitor() else {
        return;
    };
    let to_px = |v: f32| (v * scale).round() as i32;
    let origin = (
        (work.left + work.right - to_px(SIZE.0)) / 2,
        (work.top + work.bottom - to_px(SIZE.1)) / 2,
    );
    let opened = open_window(
        cx,
        Popup {
            title: "topsy settings",
            origin,
            size: SIZE,
            scale,
            activate: true,
            shape: Shape::Panel,
        },
        move |hwnd, frame, window, cx| {
            if let Some(hwnd) = hwnd {
                win32::bring_to_front(hwnd);
                // Closing it would otherwise activate the tray icon's hidden window, the last one
                // active before it. Only while it is in front, so a close from elsewhere steals nothing.
                window.on_window_should_close(cx, move |_, _| {
                    if win32::foreground_window() == hwnd {
                        win32::restore_foreground(HWND(previous as *mut _));
                    }
                    true
                });
            }
            // Moved by its top strip: float from where it was put.
            cx.observe_window_bounds(window, |view: &mut SettingsWindow, _, cx| {
                view.frame.follow_window(cx);
            })
            .detach();
            // Opened in front of everything, then an ordinary window once another is used.
            cx.observe_window_activation(window, |view: &mut SettingsWindow, window, cx| {
                if !window.is_window_active()
                    && let Some(hwnd) = view.hwnd
                {
                    cx.spawn(async move |_, _| win32::drop_topmost(hwnd))
                        .detach();
                }
            })
            .detach();
            let focus = cx.focus_handle();
            window.focus(&focus);
            let mut view = SettingsWindow {
                page: Page::Shelf,
                on: [false; Setting::ALL.len()],
                keep_days: 0,
                lineup: Lineup::new([]),
                on_change,
                frame,
                focus,
                hwnd,
                previous,
            };
            view.reload();
            view
        },
    );
    match opened {
        Ok(handle) => cx.set_global(Open(handle)),
        Err(err) => eprintln!("topsy: could not open the settings window: {err}"),
    }
}

/// A setting changed elsewhere (the menu): show it, if the window is open.
pub fn refresh(cx: &mut App) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0) {
        handle
            .update(cx, |view, _, cx| {
                let before = view.on;
                view.reload();
                if let Some(i) = Setting::ALL.iter().position(|s| *s == Setting::Black)
                    && before[i] != view.on[i]
                {
                    view.frame.set_black(view.on[i], cx);
                }
                cx.notify();
            })
            .log_err();
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frame
            .sync(SIZE.0, SIZE.1, 0.0, window.scale_factor(), false, cx);
        let viewport = window.viewport_size();
        let content = div()
            .size_full()
            .relative()
            .flex()
            .child(self.sidebar(cx))
            .child(
                div()
                    .id(("page", self.page as usize))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .px(px(22.0))
                    .pt(px(CAPTION + 8.0))
                    .child(self.page(self.page, cx)),
            )
            .child(self.caption(cx))
            .into_any_element();
        div()
            .id("settings")
            .relative()
            .w(viewport.width)
            .h(viewport.height)
            .text_color(rgb(TEXT))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .children(self.frame.draw(viewport, SIZE.1, content, None))
    }
}
