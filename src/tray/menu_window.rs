//! The tray icon's menu's windows: bare topmost popups with the notch's glass behind them.

use gpui::{
    App, Bounds, Context, TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, point, prelude::*, px, size,
};
use raw_window_handle::HasWindowHandle;
use windows::Win32::Foundation::HWND;

use crate::backdrop::Backdrop;
use crate::frame::{ACRYLIC_TINT, Frame, Glass};
use crate::{config, log, look, win32};

fn hwnd(window: &Window) -> Option<HWND> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    win32::hwnd_from_raw(handle.as_raw())
}

/// One of the menu's windows, `size` logical pixels: transparent, topmost, and either taking the
/// focus (`activate`) or never, with the notch's glass behind it. `build` makes its view.
pub(super) fn open_window<V: 'static + Render>(
    cx: &mut App,
    title: &'static str,
    origin: (i32, i32),
    size_px: (f32, f32),
    scale: f32,
    activate: bool,
    build: impl FnOnce(Option<HWND>, Frame, &mut Window, &mut Context<V>) -> V + 'static,
) -> gpui::Result<WindowHandle<V>> {
    let forced = config::get().glass;
    let mut glass = forced.unwrap_or(Glass::Blur);
    let black = forced.is_none() && look::is_black();
    let logical = |v: i32| px(v as f32 / scale);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(logical(origin.0), logical(origin.1)),
            size(px(size_px.0), px(size_px.1)),
        ))),
        // The title names it for the live checks; a bare popup shows none.
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        focus: activate,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        window_min_size: Some(size(px(1.0), px(1.0))),
        ..Default::default()
    };
    cx.open_window(options, move |window, cx| {
        let hwnd = hwnd(window);
        let mut backdrop = None;
        if let Some(hwnd) = hwnd {
            if activate {
                win32::apply_strip_styles(hwnd);
            } else {
                win32::apply_overlay_styles(hwnd);
            }
            if let Some(kind) = glass.backdrop() {
                match Backdrop::new(hwnd, kind, cx) {
                    Ok(made) => backdrop = Some(made),
                    Err(err) => {
                        log(&format!("menu glass unavailable ({err}); acrylic"));
                        glass = Glass::Acrylic;
                    }
                }
            }
            if glass == Glass::Acrylic {
                win32::round_corners(hwnd);
                if !black {
                    win32::set_acrylic(hwnd, Some(ACRYLIC_TINT));
                }
            }
        }
        let mut frame = Frame::new(hwnd, scale, glass, black, backdrop).menu();
        let to_px = |v: f32| (v * scale).round() as i32;
        let (w, h) = (to_px(size_px.0), to_px(size_px.1));
        frame.float_at((origin.0 + w / 2, origin.1 + h / 2), scale);
        cx.new(|cx| build(hwnd, frame, window, cx))
    })
}
