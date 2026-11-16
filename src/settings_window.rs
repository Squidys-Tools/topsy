//! The settings window: an ordinary, focusable window with a switch for each [`Setting`].
//! There is at most one; asking for it again raises the one that is open.

use std::rc::Rc;

use gpui::{
    App, Bounds, Context, Global, SharedString, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div,
    prelude::*, px, rgb, rgba, size,
};
use raw_window_handle::HasWindowHandle;
use windows::Win32::Foundation::HWND;

use crate::settings::Setting;
use crate::{clipboard, win32};

/// Told about every switch the window makes, after it is saved, so the app can apply it.
pub type OnChange = Rc<dyn Fn(Setting, bool, &mut App)>;

const SIZE: (f32, f32) = (340.0, 264.0);
const BACKGROUND: u32 = 0x202020;
const ACCENT: u32 = 0x4c8dff;

struct Open(WindowHandle<SettingsWindow>);

impl Global for Open {}

pub struct SettingsWindow {
    on: [bool; Setting::ALL.len()],
    keep_days: u32,
    on_change: OnChange,
}

impl SettingsWindow {
    fn reload(&mut self) {
        self.on = Setting::ALL.map(Setting::is_on);
        self.keep_days = clipboard::keep_days();
    }
}

fn hwnd(window: &Window) -> Option<HWND> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    win32::hwnd_from_raw(handle.as_raw())
}

/// Open the window, or bring the open one to the front.
pub fn open(cx: &mut App, on_change: OnChange, previous: usize) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0)
        && handle
            .update(cx, |view, window, cx| {
                view.reload();
                cx.notify();
                if let Some(hwnd) = hwnd(window) {
                    win32::bring_to_front(hwnd);
                }
            })
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(SIZE.0), px(SIZE.1)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("topsy settings".into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        focus: true,
        show: true,
        kind: WindowKind::Normal,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Opaque,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        if let Some(hwnd) = hwnd(window) {
            win32::dark_title_bar(hwnd);
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
        cx.new(|_| {
            let mut view = SettingsWindow {
                on: [false; Setting::ALL.len()],
                keep_days: 0,
                on_change,
            };
            view.reload();
            view
        })
    });
    match opened {
        Ok(handle) => cx.set_global(Open(handle)),
        Err(err) => eprintln!("topsy: could not open the settings window: {err}"),
    }
}

/// A setting changed elsewhere (the menu): show it, if the window is open.
pub fn refresh(cx: &mut App) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0) {
        let _ = handle.update(cx, |view, _, cx| {
            view.reload();
            cx.notify();
        });
    }
}

fn days(days: u32) -> String {
    match days {
        0 => "Forever".into(),
        1 => "1 day".into(),
        n => format!("{n} days"),
    }
}

fn button(id: &'static str, label: String) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px(px(10.0))
        .py(px(3.0))
        .rounded(px(6.0))
        .bg(rgba(0xffffff14))
        .hover(|button| button.bg(rgba(0xffffff24)))
        .child(label)
}

fn switch(on: bool) -> impl IntoElement {
    let track = if on { rgb(ACCENT) } else { rgb(0x3a3a3a) };
    div()
        .flex()
        .flex_none()
        .items_center()
        .w(px(36.0))
        .h(px(20.0))
        .p(px(3.0))
        .rounded_full()
        .bg(track)
        .when(on, |track| track.justify_end())
        .child(div().size(px(14.0)).rounded_full().bg(rgb(0xffffff)))
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = Setting::ALL.into_iter().enumerate().map(|(i, setting)| {
            let on = self.on[i];
            div()
                .id(i)
                .flex()
                .items_center()
                .justify_between()
                .px(px(12.0))
                .py(px(10.0))
                .rounded(px(6.0))
                .hover(|row| row.bg(rgba(0xffffff0d)))
                .child(SharedString::from(setting.label()))
                .child(switch(on))
                .on_click(cx.listener(move |view, _, _, cx| {
                    if let Some(on) = setting.toggle() {
                        view.on[i] = on;
                        cx.notify();
                        (view.on_change.clone())(setting, on, cx);
                    }
                }))
        });
        let keep = button("keep", days(self.keep_days)).on_click(cx.listener(|view, _, _, cx| {
            let choices = clipboard::DAY_CHOICES;
            let at = choices.iter().position(|&d| d == view.keep_days);
            let next = choices[at.map_or(0, |i| (i + 1) % choices.len())];
            match clipboard::set_keep_days(next) {
                Ok(()) => view.keep_days = next,
                Err(err) => eprintln!("topsy: could not change how long history is kept: {err}"),
            }
            cx.notify();
        }));
        let clear = button("clear", "Clear".into())
            .on_click(|_, _, _| clipboard::send(clipboard::Command::Clear));
        let retention = div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(12.0))
            .py(px(8.0))
            .child("Keep clipboard for")
            .child(div().flex().gap(px(6.0)).child(keep).child(clear));
        let quit = button("quit", "Quit topsy".into()).on_click(|_, _, cx| cx.quit());
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_between()
            .p(px(8.0))
            .bg(rgb(BACKGROUND))
            .text_color(rgb(0xffffff))
            .text_sm()
            .child(div().flex().flex_col().children(rows).child(retention))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(12.0))
                    .py(px(8.0))
                    .child(
                        div()
                            .text_color(rgba(0xffffff80))
                            .child(concat!("topsy ", env!("CARGO_PKG_VERSION"))),
                    )
                    .child(quit),
            )
    }
}
