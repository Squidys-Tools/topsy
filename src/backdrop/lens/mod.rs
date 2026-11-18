//! The open notch's lens glass: what is behind the notch, bent by a soft round bevel at its
//! edges (docs/internals/glass.md). The compositor's effects can't displace pixels, so this glass
//! is our own shader, drawn with Direct3D 11 into a click-through window of its own just under
//! the island. The island's compositor glass steps aside while it shows.
//!
//! It reads the screen once as the notch opens. With the *Live glass* setting it then follows a
//! live copy of what is behind, which leaves topsy out, so both show in screenshots and
//! recordings.

mod capture;
mod draw;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct3D11::{D3D11_BOX, D3D11_TEXTURE2D_DESC};
use windows::Win32::Graphics::Gdi::{
    CreateRoundRectRgn, GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
    SetWindowRgn,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, HTTRANSPARENT, RegisterClassW, SWP_HIDEWINDOW, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowPos, WM_NCHITTEST, WNDCLASSW,
    WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Error, w};

use self::capture::{Capture, Mirror, failed, snapshot};
use self::draw::{Painter, Source};
use super::Outline;
use super::live::Shared;
use crate::log::{LogErr, log};
use model::float::Rect;
use model::lens::source;

unsafe extern "system" fn lens_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCHITTEST {
        return LRESULT(HTTRANSPARENT as isize);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// The lens of one notch; a cheap handle to share with the tasks that reshape it.
#[derive(Clone)]
pub struct Lens(Rc<Inner>);

struct Inner {
    island: HWND,
    window: HWND,
    /// What the island last asked for. Starting and stopping the capture pumps messages, so
    /// GPUI can run a frame in the middle and ask again; that waits here for the running call.
    wanted: Cell<Wanted>,
    /// The last show failed, so the compositor glass should show instead.
    failed: Cell<bool>,
    shown: Cell<bool>,
    state: RefCell<State>,
}

#[derive(Clone, Copy, PartialEq)]
struct Wanted {
    placed: Option<Placed>,
    live: bool,
}

/// Where the open notch is, in physical pixels.
#[derive(Clone, Copy, PartialEq)]
struct Placed {
    origin: (i32, i32),
    size: (i32, i32),
    scale: f32,
    outline: Outline,
}

struct State {
    island: HWND,
    window: HWND,
    painter: Painter,
    source: Option<Source>,
    /// What the live glass made; without it the lens can't follow the screen.
    shared: Option<Shared>,
    mirror: Option<Mirror>,
    capture: Option<Capture>,
    applied: Wanted,
    placed: Option<Placed>,
    frames: async_channel::Sender<()>,
}

impl Lens {
    pub fn new(
        island: HWND,
        live: bool,
        shared: Option<Shared>,
        cx: &mut gpui::App,
    ) -> windows::core::Result<Self> {
        let window = lens_window()?;
        let (frames, woken) = async_channel::bounded(1);
        let applied = Wanted { placed: None, live };
        let lens = Lens(Rc::new(Inner {
            island,
            window,
            wanted: Cell::new(applied),
            failed: Cell::new(false),
            shown: Cell::new(false),
            state: RefCell::new(State {
                island,
                window,
                painter: Painter::new(window)?,
                source: None,
                shared,
                mirror: None,
                capture: None,
                applied,
                placed: None,
                frames,
            }),
        }));
        let handle = lens.clone();
        cx.spawn(async move |_| {
            while woken.recv().await.is_ok() {
                // Busy: a frame is skipped, and the next one comes soon.
                if let Ok(mut state) = handle.0.state.try_borrow_mut() {
                    state.follow_screen();
                }
            }
        })
        .detach();
        Ok(lens)
    }

    /// Show the lens for a window at `origin` of `size` (physical screen pixels), shaped to
    /// `outline`. False if it can't draw, where the compositor glass should show instead.
    pub fn show(&self, origin: (i32, i32), size: (i32, i32), scale: f32, outline: Outline) -> bool {
        let placed = Placed {
            origin,
            size,
            scale,
            outline,
        };
        self.ask(|wanted| wanted.placed = Some(placed));
        !self.0.failed.get()
    }

    pub fn hide(&self) {
        self.ask(|wanted| wanted.placed = None);
    }

    /// The *Live glass* setting; takes effect at once if the notch is open.
    pub fn set_live(&self, live: bool) {
        self.ask(|wanted| wanted.live = live);
    }

    /// Back under the island, after the island has been raised.
    pub fn restack(&self) {
        let inner = &*self.0;
        if inner.shown.get() {
            let keep = SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE;
            unsafe { SetWindowPos(inner.window, Some(inner.island), 0, 0, 0, 0, keep) }.log_err();
        }
    }

    /// Record the change, then apply what is wanted unless a call is already doing so.
    fn ask(&self, change: impl FnOnce(&mut Wanted)) {
        let inner = &*self.0;
        let mut wanted = inner.wanted.get();
        change(&mut wanted);
        inner.wanted.set(wanted);
        let Ok(mut state) = inner.state.try_borrow_mut() else {
            return;
        };
        while state.applied != inner.wanted.get() {
            let wanted = inner.wanted.get();
            let was = std::mem::replace(&mut state.applied, wanted);
            let result = state.apply(was, wanted);
            if let Err(e) = &result {
                log(&format!("lens glass failed ({e}); compositor glass"));
                state.hide();
            }
            inner.failed.set(result.is_err());
            inner.shown.set(state.placed.is_some());
        }
    }
}

/// A window that never activates. Its region keeps it to the notch's body, which the island
/// covers, so it never takes a click. (A layered, click-through window would, but
/// DirectComposition content on one doesn't show.)
fn lens_window() -> windows::core::Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("topsy lens");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(lens_proc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        });
        let window = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            class,
            w!("topsy lens"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        Ok(window)
    }
}

impl State {
    fn apply(&mut self, was: Wanted, wanted: Wanted) -> windows::core::Result<()> {
        let Some(placed) = wanted.placed else {
            self.hide();
            return Ok(());
        };
        let opening = self.placed.is_none();
        if self.placed != Some(placed) {
            self.place(placed)?;
        }
        // Tried as it opens or as the setting turns on, not again on every frame of the spring.
        if !wanted.live {
            self.stop_live();
        } else if self.capture.is_none() && (opening || !was.live) {
            self.start_live().log_err();
        }
        Ok(())
    }

    fn place(&mut self, placed: Placed) -> windows::core::Result<()> {
        let opening = self.placed.is_none();
        self.placed = Some(placed);
        if opening {
            // Read before the lens shows, and while the notch is still small: what is behind
            // the open notch, with nothing of ours over it but the black at the top.
            self.read_screen(placed)?;
        }
        let source = self.source.as_ref().ok_or_else(failed)?;
        self.painter.draw(source, placed)?;
        let (x, y) = placed.origin;
        let (width, height) = placed.size;
        let show = if opening {
            SWP_SHOWWINDOW
        } else {
            Default::default()
        };
        let Outline {
            x: left,
            y: top,
            width: body_width,
            height: body_height,
            radius,
        } = placed.outline;
        let corner = (2.0 * radius).round() as i32;
        unsafe {
            // Just under the island, wherever it is in the topmost band.
            SetWindowPos(
                self.window,
                Some(self.island),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | show,
            )?;
            let region = CreateRoundRectRgn(
                left.floor() as i32,
                top.floor() as i32,
                (left + body_width).ceil() as i32 + 1,
                (top + body_height).ceil() as i32 + 1,
                corner,
                corner,
            );
            // The window owns the region from here on.
            SetWindowRgn(self.window, Some(region), true);
        }
        Ok(())
    }

    fn hide(&mut self) {
        if self.placed.take().is_none() {
            return;
        }
        self.stop_live();
        unsafe {
            SetWindowPos(
                self.window,
                None,
                0,
                0,
                0,
                0,
                SWP_HIDEWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
            )
        }
        .log_err();
    }

    fn monitor(&self) -> windows::core::Result<Rect> {
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            let monitor = MonitorFromWindow(self.island, MONITOR_DEFAULTTOPRIMARY);
            GetMonitorInfoW(monitor, &mut info).ok()?;
            let r = info.rcMonitor;
            Ok(Rect {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            })
        }
    }

    /// Read the screen behind the open notch into the source texture.
    fn read_screen(&mut self, placed: Placed) -> windows::core::Result<()> {
        let monitor = self.monitor()?;
        let center_x = placed.origin.0 + placed.size.0 / 2;
        let rect = source(center_x, placed.origin.1, placed.scale, monitor);
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return Err(failed());
        }
        let pixels = snapshot(rect)?;
        let size = |r: Rect| (r.right - r.left, r.bottom - r.top);
        let reusable = self.source.take().filter(|s| size(s.rect) == size(rect));
        let source = match reusable {
            Some(source) => Source { rect, ..source },
            None => self.painter.source(rect)?,
        };
        self.painter.upload(&source, &pixels);
        self.source = Some(source);
        Ok(())
    }

    fn start_live(&mut self) -> windows::core::Result<()> {
        let (Some(shared), Some(source)) = (&self.shared, &self.source) else {
            return Err(Error::new(
                E_FAIL,
                "live glass needs the live compositor glass",
            ));
        };
        let mirror = match self.mirror.take() {
            Some(mirror) => mirror,
            None => Mirror::new(shared)?,
        };
        let device = &self.painter.capture_device;
        let capture = mirror.capture(
            source.rect,
            [self.island, self.window],
            device,
            self.frames.clone(),
        );
        self.mirror = Some(mirror);
        self.capture = Some(capture?);
        Ok(())
    }

    fn stop_live(&mut self) {
        if self.capture.take().is_some()
            && let Some(mirror) = &self.mirror
        {
            mirror.hide();
        }
    }

    /// The live copy has a new frame: copy it in and redraw.
    fn follow_screen(&mut self) {
        let (Some(capture), Some(placed), Some(source)) =
            (&self.capture, self.placed, &self.source)
        else {
            return;
        };
        let mut changed = false;
        let read = capture.newest(|frame| {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { frame.GetDesc(&mut desc) };
            let from = D3D11_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: desc
                    .Width
                    .min((source.rect.right - source.rect.left) as u32),
                bottom: desc
                    .Height
                    .min((source.rect.bottom - source.rect.top) as u32),
                back: 1,
            };
            unsafe {
                self.painter.context.CopySubresourceRegion(
                    &source.texture,
                    0,
                    0,
                    0,
                    0,
                    frame,
                    0,
                    Some(&from),
                );
            }
            self.painter.filled(source);
            changed = true;
        });
        read.log_err();
        if changed {
            self.painter.draw(source, placed).log_err();
        }
    }
}
