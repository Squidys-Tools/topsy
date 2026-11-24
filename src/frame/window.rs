//! A glass window of its own: a bare topmost popup with the notch's glass behind it, floating
//! as the tray icon's menu does, or hanging from the top edge as the clipboard palette does;
//! and the windows made at launch (the island's, the basket's, the floating player's).

use gpui::{
    App, Bounds, Context, Entity, Pixels, TitlebarOptions, Window, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowKind, WindowOptions, point, prelude::*, px, size,
};
use raw_window_handle::HasWindowHandle;
use windows::Win32::Foundation::HWND;

use super::{ACRYLIC_TINT, Frame, Glass};
use crate::backdrop::Backdrop;
use crate::win32::DropEvent;
use crate::{config, log, look, win32};

fn hwnd(window: &Window) -> Option<HWND> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    win32::hwnd_from_raw(handle.as_raw())
}

/// How an [`open_window`] sits and is glazed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    /// Centred where it opens, evenly glazed like a menu.
    Floating,
    /// Hanging from the top edge of the monitor it opens on, a notch of its own.
    Hanging,
}

/// What [`open_window`] makes.
pub(crate) struct Popup {
    /// Names it for the live checks; a bare popup shows none.
    pub title: &'static str,
    /// Physical screen pixels, on a monitor of `scale`.
    pub origin: (i32, i32),
    /// Logical pixels.
    pub size: (f32, f32),
    pub scale: f32,
    /// Takes the focus, or never does.
    pub activate: bool,
    pub shape: Shape,
}

/// A transparent, topmost glass window, as `popup` says. `build` makes its view.
pub(crate) fn open_window<V: 'static + Render>(
    cx: &mut App,
    popup: Popup,
    build: impl FnOnce(Option<HWND>, Frame, &mut Window, &mut Context<V>) -> V + 'static,
) -> gpui::Result<WindowHandle<V>> {
    let Popup {
        title,
        origin,
        size: size_px,
        scale,
        activate,
        shape,
    } = popup;
    let forced = config::get().glass;
    let mut glass = forced.unwrap_or(Glass::Blur);
    let black = forced.is_none() && look::is_black();
    let logical = |v: i32| px(v as f32 / scale);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(logical(origin.0), logical(origin.1)),
            size(px(size_px.0), px(size_px.1)),
        ))),
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
                win32::apply_focus_styles(hwnd);
            } else {
                win32::apply_overlay_styles(hwnd);
            }
            if let Some(kind) = glass.backdrop() {
                match Backdrop::new(hwnd, kind, cx) {
                    Ok(made) => backdrop = Some(made),
                    Err(err) => {
                        log(&format!("{title} glass unavailable ({err}); acrylic"));
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
        let frame = Frame::new(hwnd, scale, glass, black, backdrop);
        let frame = match shape {
            Shape::Floating => {
                let mut frame = frame.menu();
                let to_px = |v: f32| (v * scale).round() as i32;
                let (w, h) = (to_px(size_px.0), to_px(size_px.1));
                frame.float_at((origin.0 + w / 2, origin.1 + h / 2), scale);
                frame
            }
            Shape::Hanging => frame.hanging(),
        };
        cx.new(|cx| build(hwnd, frame, window, cx))
    })
}

/// Open one of topsy's glass windows: transparent, never activated, click-through until it is
/// given a shape, with the compositor's blur behind it (or acrylic where that can't start), and
/// a drop target that reports refused drags on `refusals`, if given. `build` makes its view.
pub(crate) fn open_glass<V: 'static + Render>(
    cx: &mut App,
    bounds: Bounds<Pixels>,
    mut glass: Glass,
    black: bool,
    refusals: Option<async_channel::Sender<DropEvent>>,
    previous_foreground: HWND,
    build: impl FnOnce(Option<HWND>, Glass, Option<Backdrop>, &mut Window, &mut App) -> Entity<V>
    + 'static,
) -> gpui::Result<gpui::WindowHandle<V>> {
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            // `appears_transparent: false` stops GPUI insetting the client area by an
            // invisible resize frame; `apply_overlay_styles` then removes the caption
            // that Windows would otherwise add.
            titlebar: Some(TitlebarOptions {
                title: None,
                appears_transparent: false,
                traffic_light_position: None,
            }),
            window_min_size: Some(size(px(1.0), px(1.0))),
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            // Only lets a `WindowControlArea::Drag` element be a caption (the basket's handle);
            // every other hit test is still Windows' own.
            is_movable: true,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..Default::default()
        },
        move |window, cx| {
            let hwnd = window
                .window_handle()
                .ok()
                .and_then(|h| win32::hwnd_from_raw(h.as_raw()));
            let mut backdrop = None;
            if let Some(hwnd) = hwnd {
                win32::apply_overlay_styles(hwnd);
                if let Some(kind) = glass.backdrop() {
                    match Backdrop::new(hwnd, kind, cx) {
                        Ok(b) => backdrop = Some(b),
                        Err(e) => {
                            log(&format!("compositor glass unavailable ({e}); acrylic"));
                            glass = Glass::Acrylic;
                        }
                    }
                }
                if glass == Glass::Acrylic {
                    win32::round_corners(hwnd);
                    // After GPUI has applied its own background appearance, so ours wins.
                    if !black {
                        win32::set_acrylic(hwnd, Some(ACRYLIC_TINT));
                    }
                }
                if let Some(refusals) = refusals {
                    win32::watch_drop_refusals(hwnd, refusals);
                }
                win32::restore_foreground(previous_foreground);
            }
            build(hwnd, glass, backdrop, window, cx)
        },
    )
}
