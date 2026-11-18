//! The result cards, after Droppy's: when a run starts, the panel steps aside for a column of
//! cards where it stood, one per file (one for a zip, a PDF or a rename). A card shows its file
//! blurred while it waits or works, with a stop button and its progress, then sharp, with the
//! size before and after. They go by themselves a few seconds after the last is done, unless
//! the cursor is on them. A finished card copies its file, to paste in a mail or a chat. They
//! can be dragged anywhere by the card. Like the island, they never take the focus.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, AnyWindowHandle, App, AsyncApp, Bounds, ClickEvent,
    Context, FontWeight, ObjectFit, Render, RenderImage, Stateful, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div, img,
    linear_color_stop, linear_gradient, point, prelude::*, px, rgb, rgba, size,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::COINIT_MULTITHREADED;

use crate::{icon, win32};
use model::float::Rect;
use model::transform::Action;
use model::transform_card::{Headline, Made};

mod view;

const CARD_W: f32 = 208.0;
const CARD_H: f32 = 156.0;
const GAP: f32 = 10.0;
const RADIUS: f32 = 20.0;
const MONO: &str = "Consolas";
/// How long the cards stay once the last is done.
const LINGER: Duration = Duration::from_secs(6);
const FADE: Duration = Duration::from_millis(200);

enum Status {
    Waiting,
    /// `label` replaces the action's own word, as while a tool is fetched.
    Working {
        label: Option<String>,
        progress: f32,
    },
    Done {
        made: Made,
        output: PathBuf,
    },
}

struct Card {
    source: PathBuf,
    status: Status,
    sharp: Option<Arc<RenderImage>>,
    soft: Option<Arc<RenderImage>>,
    gone: bool,
    /// Its file is on the clipboard.
    copied: bool,
}

pub struct Cards {
    action: Action,
    cards: Vec<Card>,
    cancel: Arc<AtomicBool>,
    window: Option<AnyWindowHandle>,
    hwnd: usize,
    scale: f32,
    finished: bool,
    hovered: bool,
    leaving: bool,
    /// Bumped by each wait to leave, so only the latest one counts.
    wait: u32,
}

/// Open the cards for `sources`, their right edge on `anchor`'s, stacked up from its bottom. The
/// caller has hidden the panel; whatever has the focus now keeps it.
pub fn open(
    cx: &mut App,
    anchor: Rect,
    action: Action,
    sources: Vec<PathBuf>,
    cancel: Arc<AtomicBool>,
) -> Option<WindowHandle<Cards>> {
    let (_, work, scale) = win32::cursor_monitor()?;
    let sources = if action.combines() || action == Action::Rename {
        sources.into_iter().take(1).collect()
    } else {
        sources
    };
    let w = (CARD_W * scale).round() as i32;
    let x = anchor.right - w;
    let bottom = anchor.bottom.min(work.bottom);
    let h = column_height(sources.len(), scale).min(bottom - work.top);
    let y = bottom - h;
    let logical = |v: i32| px(v as f32 / scale);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(logical(x), logical(y)),
            size(logical(w), logical(h)),
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Transform results".into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: true,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        window_min_size: Some(size(px(1.0), px(1.0))),
        ..Default::default()
    };
    let previous = win32::foreground_window();
    let handle = cx
        .open_window(options, |window, cx| {
            let hwnd = super::hwnd(window);
            if let Some(hwnd) = hwnd {
                win32::apply_overlay_styles(hwnd);
                let (raw, previous) = (hwnd.0 as usize, previous.0 as usize);
                let shape = column_clip(sources.len(), scale, h);
                cx.defer(move |_| {
                    win32::move_window(HWND(raw as *mut _), x, y, w, h, true);
                    win32::set_clip(HWND(raw as *mut _), shape);
                    win32::restore_foreground(HWND(previous as *mut _));
                });
            }
            cx.new(|cx| {
                let mut cards = Cards {
                    action,
                    cards: Vec::new(),
                    cancel,
                    window: None,
                    hwnd: hwnd.map_or(0, |h| h.0 as usize),
                    scale,
                    finished: false,
                    hovered: false,
                    leaving: false,
                    wait: 0,
                };
                cards.load(sources, cx);
                cards
            })
        })
        .inspect_err(|err| eprintln!("topsy: could not open the result cards: {err}"))
        .ok()?;
    let any = handle.into();
    handle
        .update(cx, |cards, _, _| cards.window = Some(any))
        .ok();
    Some(handle)
}

/// The physical height of a column of `n` cards.
fn column_height(n: usize, scale: f32) -> i32 {
    let n = n.max(1) as f32;
    ((n * CARD_H + (n - 1.0) * GAP) * scale).round() as i32
}

/// The cards' region, so the gaps and corners between them are click-through. A column
/// taller than the window scrolls, which a fixed region can't follow, so it takes it whole.
fn column_clip(n: usize, scale: f32, window_height: i32) -> win32::Clip {
    if column_height(n, scale) > window_height {
        return win32::Clip::Everything;
    }
    let px = |v: f32| (v * scale).round() as i32;
    win32::Clip::Column {
        width: px(CARD_W),
        height: px(CARD_H),
        tops: (0..n).map(|i| px(i as f32 * (CARD_H + GAP))).collect(),
        radius: px(RADIUS),
    }
}

impl Cards {
    /// The cards, then their pictures, read off the UI thread: sharp for a finished card,
    /// blurred while it waits.
    fn load(&mut self, sources: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.cards = (sources.iter())
            .map(|source| Card {
                source: source.clone(),
                status: Status::Waiting,
                sharp: None,
                soft: None,
                gone: false,
                copied: false,
            })
            .collect();
        let big = (CARD_W * self.scale).round() as i32;
        let (tx, rx) = async_channel::unbounded();
        std::thread::spawn(move || {
            win32::with_com(COINIT_MULTITHREADED, || {
                for (i, path) in sources.iter().enumerate() {
                    let aspect = CARD_W / CARD_H;
                    let soft = icon::load_cover(path, 48, aspect, Some(2.5));
                    let sharp = icon::load_cover(path, big, aspect, None);
                    if tx.send_blocking((i, soft, sharp)).is_err() {
                        break;
                    }
                }
            })
        });
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            while let Ok((i, soft, sharp)) = rx.recv().await {
                let live = this.update(cx, |this, cx| {
                    if let Some(card) = this.cards.get_mut(i) {
                        card.soft = soft;
                        card.sharp = sharp;
                        cx.notify();
                    }
                });
                if live.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn step(
        &mut self,
        index: usize,
        label: Option<String>,
        progress: f32,
        cx: &mut Context<Self>,
    ) {
        if let Some(card) = self.cards.get_mut(index) {
            card.status = Status::Working { label, progress };
            cx.notify();
        }
    }

    pub fn made(&mut self, index: usize, made: Made, output: PathBuf, cx: &mut Context<Self>) {
        if let Some(card) = self.cards.get_mut(index) {
            card.status = Status::Done { made, output };
            cx.notify();
        }
    }

    /// The run is done: a card that made nothing to show goes, and the rest leave in a while.
    pub fn finish(&mut self, cx: &mut Context<Self>) {
        self.finished = true;
        for card in &mut self.cards {
            card.gone |= !matches!(card.status, Status::Done { .. });
        }
        self.shown_changed(cx);
        self.wait_to_leave(cx);
    }

    fn wait_to_leave(&mut self, cx: &mut Context<Self>) {
        self.wait += 1;
        let wait = self.wait;
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            cx.background_executor().timer(LINGER).await;
            this.update(cx, |this, cx| {
                if this.wait != wait || this.hovered {
                    return;
                }
                // Cards that opened under a still cursor hear no hover until it moves.
                if win32::cursor_over(HWND(this.hwnd as *mut _)) {
                    this.wait_to_leave(cx);
                } else {
                    this.leave(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Fade out, then close.
    fn leave(&mut self, cx: &mut Context<Self>) {
        if self.leaving {
            return;
        }
        self.leaving = true;
        cx.notify();
        let window = self.window;
        cx.spawn(async move |_, cx: &mut AsyncApp| {
            cx.background_executor().timer(FADE).await;
            if let Some(window) = window {
                window
                    .update(cx, |_, window, _| window.remove_window())
                    .ok();
            }
        })
        .detach();
    }

    /// Close at once, as when the run failed or was stopped and the panel comes back.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if let Some(window) = self.window {
            cx.defer(move |cx| {
                window
                    .update(cx, |_, window, _| window.remove_window())
                    .ok();
            });
        }
    }

    /// Put a finished card's file on the clipboard, off the UI thread: opening the clipboard
    /// waits on whoever has it.
    fn copy(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(Status::Done { output, .. }) = self.cards.get(index).map(|c| &c.status) else {
            return;
        };
        let (data, owner) = (win32::paste::Data::Files(vec![output.clone()]), self.hwnd);
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            let put = (cx.background_executor())
                .spawn(async move { win32::paste::put_owned(&data, HWND(owner as *mut _)) })
                .await;
            this.update(cx, |this, cx| {
                if let Some(card) = this.cards.get_mut(index) {
                    card.copied = put;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn dismiss(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(card) = self.cards.get_mut(index) {
            card.gone = true;
        }
        self.shown_changed(cx);
    }

    /// Fit the window to the cards still shown, so the space a dismissed one left takes no
    /// clicks; with none left, go.
    fn shown_changed(&mut self, cx: &mut Context<Self>) {
        let shown = self.cards.iter().filter(|c| !c.gone).count();
        if shown == 0 {
            return self.leave(cx);
        }
        let (raw, scale) = (self.hwnd, self.scale);
        let Some(rect) = win32::window_rect(HWND(raw as *mut _)) else {
            return;
        };
        let h = column_height(shown, scale).min(rect.bottom - rect.top);
        let shape = column_clip(shown, scale, h);
        cx.spawn(async move |_, _: &mut AsyncApp| {
            let w = rect.right - rect.left;
            win32::move_window(HWND(raw as *mut _), rect.left, rect.bottom - h, w, h, false);
            win32::set_clip(HWND(raw as *mut _), shape);
        })
        .detach();
        cx.notify();
    }

    fn hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.hovered = hovered;
        if !hovered && self.finished {
            self.wait_to_leave(cx);
        }
    }
}
