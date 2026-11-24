//! The lens glass: what is behind a window, bent by a soft round bevel at its edges
//! (docs/internals/glass.md). The compositor's effects can't displace pixels, so this glass is
//! our own shader, drawn with Direct3D 11 into a window of its own that never takes a click. The
//! window's compositor glass steps aside while it shows.
//!
//! The open notch's lens reads the screen once as the notch opens, and with the *Live glass*
//! setting then follows a live copy of what is behind. The floating player's always follows it,
//! slowly until the pointer is over it, since a window that moves can't keep a reading. The copy
//! leaves topsy out, so both show in screenshots and recordings.

mod capture;
mod draw;
mod window;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use windows::Win32::Foundation::{E_FAIL, HWND};
use windows::Win32::Graphics::Direct3D11::{D3D11_BOX, D3D11_TEXTURE2D_DESC};
use windows::core::Error;

pub(super) use self::capture::park as park_mirror;
use self::capture::{Capture, Mirror, failed, snapshot};
use self::draw::{Painter, Source};
use super::Outline;
use super::live::Shared;
use crate::log::{LogErr, log};
use model::float::Rect;
use model::lens::{around, holds, source};

/// How often the open notch's live lens follows the screen.
const NOTCH_FPS: u32 = 30;
/// How often a floating window's lens follows the screen: while the pointer is over it (and so
/// while it is dragged), and otherwise.
const ACTIVE_FPS: u32 = 24;
const RESTING_FPS: u32 = 5;
/// Room around a floating window in what its lens reads, logical pixels: moves within it only
/// shift the reading, and the bevel bends in from up to its strength beyond the edge.
const SLACK: f32 = 160.0;
const MARGIN: f32 = 48.0;

/// The window a lens is behind.
#[derive(Clone, Copy, PartialEq)]
pub enum Holder {
    /// The island, whose open notch the lens sits under in a window of its own.
    Notch,
    /// A floating window of at most `reach` (logical pixels), whose lens is a child window: it
    /// moves with it in the same step, and the window's content draws over it.
    Floating { reach: (f32, f32) },
}

/// The lens of one window; a cheap handle to share with the tasks that reshape it.
#[derive(Clone)]
pub struct Lens(Rc<Inner>);

struct Inner {
    island: HWND,
    window: HWND,
    holder: Holder,
    /// What the window last asked for. Starting and stopping the capture pumps messages, so
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
    /// The pointer is over a floating window, so its lens follows the screen at full rate.
    active: bool,
}

/// Where the window is, in physical pixels.
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
    holder: Holder,
    painter: Painter,
    /// The screen read, and the part of the screen it holds.
    source: Option<Source>,
    /// What the live glass made; without it the lens can't follow the screen.
    shared: Option<Shared>,
    mirror: Option<Mirror>,
    capture: Option<Capture>,
    /// The part of the screen the live copy is pointed at; the source takes it with the next frame.
    aimed: Option<Rect>,
    /// A floating lens's capture is to start from the lens's own task.
    start_pending: bool,
    applied: Wanted,
    placed: Option<Placed>,
    frames: async_channel::Sender<()>,
}

impl Lens {
    pub fn new(
        island: HWND,
        holder: Holder,
        live: bool,
        shared: Option<Shared>,
        cx: &mut gpui::App,
    ) -> windows::core::Result<Self> {
        let parent = match holder {
            Holder::Notch => None,
            Holder::Floating { .. } => Some(island),
        };
        let window = window::create(parent)?;
        let painter = draw::gpu(cx).and_then(|gpu| Painter::new(gpu, window));
        let painter = match painter {
            Ok(painter) => painter,
            Err(e) => {
                window::destroy(window);
                return Err(e);
            }
        };
        let (frames, woken) = async_channel::bounded(1);
        let applied = Wanted {
            placed: None,
            live,
            active: false,
        };
        let lens = Lens(Rc::new(Inner {
            island,
            window,
            holder,
            wanted: Cell::new(applied),
            failed: Cell::new(false),
            shown: Cell::new(false),
            state: RefCell::new(State {
                island,
                window,
                holder,
                painter,
                source: None,
                shared,
                mirror: None,
                capture: None,
                aimed: None,
                start_pending: false,
                applied,
                placed: None,
                frames,
            }),
        }));
        let handle = lens.clone();
        cx.spawn(async move |_| {
            while woken.recv().await.is_ok() {
                // Busy: a frame is skipped, and the next one comes soon. A start waits for the
                // call that is busy, which wakes this again.
                if let Ok(mut state) = handle.0.state.try_borrow_mut() {
                    state.start_pending();
                    state.follow_screen();
                }
                handle.settle();
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

    /// Whether the pointer is over the floating window.
    pub fn set_active(&self, active: bool) {
        self.ask(|wanted| wanted.active = active);
    }

    /// Back under the island, after the island has been raised. A child lens stays put.
    pub fn restack(&self) {
        let inner = &*self.0;
        if inner.shown.get() && inner.holder == Holder::Notch {
            window::restack(inner.window, inner.island);
        }
    }

    /// Record the change, then apply what is wanted unless a call is already doing so.
    fn ask(&self, change: impl FnOnce(&mut Wanted)) {
        let inner = &*self.0;
        let mut wanted = inner.wanted.get();
        change(&mut wanted);
        inner.wanted.set(wanted);
        self.settle();
    }

    /// Apply what is wanted, unless a call is already doing so.
    fn settle(&self) {
        let inner = &*self.0;
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
        // The task may have woken while this was busy, and found nothing it could do.
        if state.start_pending {
            state.wake();
        }
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
            match self.holder {
                Holder::Notch => {
                    self.start_live(wanted).log_err();
                }
                // Starting takes tens of milliseconds and pumps messages: a window pulled out
                // shows at once and goes to the move loop, and the reading is followed after.
                Holder::Floating { .. } => {
                    self.start_pending = true;
                    self.wake();
                }
            }
        } else if let Some(capture) = &self.capture
            && wanted.active != was.active
        {
            capture.pace(self.fps(wanted));
        }
        Ok(())
    }

    fn fps(&self, wanted: Wanted) -> u32 {
        match (self.holder, wanted.active) {
            (Holder::Notch, _) => NOTCH_FPS,
            (Holder::Floating { .. }, true) => ACTIVE_FPS,
            (Holder::Floating { .. }, false) => RESTING_FPS,
        }
    }

    fn place(&mut self, placed: Placed) -> windows::core::Result<()> {
        let opening = self.placed.is_none();
        self.placed = Some(placed);
        if opening {
            // Read before the lens shows, and while the window is still small or hidden: what
            // is behind it, with nothing of ours over it but the notch's black top.
            self.read_screen(placed)?;
        } else if let Holder::Floating { .. } = self.holder
            && self.capture.is_none()
            && !self.start_pending
        {
            // It couldn't follow the screen, and the reading goes stale as the window moves.
            return Err(failed());
        } else {
            self.follow_window(placed)?;
        }
        let source = self.source.as_ref().ok_or_else(failed)?;
        self.painter.draw(source, placed)?;
        match self.holder {
            Holder::Notch => window::place_under(self.window, self.island, placed, opening),
            Holder::Floating { .. } => window::fill(self.window, placed.size, opening),
        }
    }

    fn hide(&mut self) {
        if self.placed.take().is_none() {
            return;
        }
        self.stop_live();
        window::hide(self.window);
    }

    /// The part of the screen the lens reads for a window at `placed`.
    fn reading(&self, placed: Placed) -> windows::core::Result<Rect> {
        let center = (
            placed.origin.0 + placed.size.0 / 2,
            placed.origin.1 + placed.size.1 / 2,
        );
        let rect = match self.holder {
            Holder::Notch => {
                let monitor = window::monitor_of(self.island)?;
                source(center.0, placed.origin.1, placed.scale, monitor)
            }
            Holder::Floating { reach } => {
                let monitor = window::monitor_at(center)?;
                let px = |v: f32| ((v + 2.0 * SLACK) * placed.scale).round() as i32;
                around(center, (px(reach.0), px(reach.1)), monitor)
            }
        };
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return Err(failed());
        }
        Ok(rect)
    }

    /// Read the screen behind the opening window into the source texture.
    fn read_screen(&mut self, placed: Placed) -> windows::core::Result<()> {
        let rect = self.reading(placed)?;
        let pixels = snapshot(rect)?;
        let size = |r: Rect| (r.right - r.left, r.bottom - r.top);
        let reusable = self.source.take().filter(|s| size(s.rect) == size(rect));
        let gpu = &self.painter.gpu;
        let source = match reusable {
            Some(source) => Source { rect, ..source },
            None => gpu.source(rect)?,
        };
        gpu.upload(&source, &pixels);
        self.source = Some(source);
        Ok(())
    }

    /// A floating window moved or changed size: point the live copy at where it is now, once it
    /// has moved too far for the reading to hold it. Until the next frame the old reading is
    /// drawn shifted, which keeps whatever behind it is still.
    fn follow_window(&mut self, placed: Placed) -> windows::core::Result<()> {
        let (Holder::Floating { .. }, Some(aimed), Some(mirror)) =
            (self.holder, self.aimed, &self.mirror)
        else {
            return Ok(());
        };
        let margin = (MARGIN * placed.scale).round() as i32;
        if holds(aimed, placed.origin, placed.size, margin) {
            return Ok(());
        }
        let rect = self.reading(placed)?;
        if rect != aimed {
            mirror.aim(rect, [self.island, self.window])?;
            self.aimed = Some(rect);
        }
        Ok(())
    }

    fn start_live(&mut self, wanted: Wanted) -> windows::core::Result<()> {
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
        let device = &self.painter.gpu.capture_device;
        let capture = mirror.capture(
            source.rect,
            [self.island, self.window],
            device,
            self.frames.clone(),
            self.fps(wanted),
        );
        self.mirror = Some(mirror);
        self.capture = Some(capture?);
        self.aimed = Some(source.rect);
        Ok(())
    }

    /// Start the capture a floating lens asked for, if it is still showing.
    fn start_pending(&mut self) {
        if !std::mem::take(&mut self.start_pending) || self.placed.is_none() {
            return;
        }
        if let Err(e) = self.start_live(self.applied) {
            log(&format!("floating lens can't follow the screen ({e})"));
        }
    }

    /// Wake the lens's task.
    fn wake(&self) {
        match self.frames.try_send(()) {
            // Full: it is already woken.
            Ok(()) | Err(async_channel::TrySendError::Full(())) => {}
            Err(async_channel::TrySendError::Closed(())) => log("lens task ended"),
        }
    }

    fn stop_live(&mut self) {
        self.aimed = None;
        self.start_pending = false;
        if self.capture.take().is_some()
            && let Some(mirror) = &self.mirror
        {
            mirror.hide();
        }
    }

    /// The live copy has a new frame: copy it in and redraw.
    fn follow_screen(&mut self) {
        let (Some(capture), Some(placed), Some(source)) =
            (&self.capture, self.placed, &mut self.source)
        else {
            return;
        };
        let gpu = &self.painter.gpu;
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
                gpu.context.CopySubresourceRegion(
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
            gpu.filled(source);
            changed = true;
        });
        read.log_err();
        if changed {
            if let Some(aimed) = self.aimed {
                source.rect = aimed;
            }
            self.painter.draw(source, placed).log_err();
        }
    }
}
