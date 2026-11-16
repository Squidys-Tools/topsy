//! The result cards, after Droppy's: when a run starts, the panel steps aside for a column of
//! cards where it stood, one per file (one for a zip, a PDF or a rename). A card shows its file
//! blurred while it waits or works, with a stop button and its progress, then sharp, with the
//! size before and after. They go by themselves a few seconds after the last is done, unless
//! the cursor is on them. Like the island, they never take the focus.

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

use super::widgets::ICONS;
use crate::model::float::Rect;
use crate::model::transform::Action;
use crate::model::transform_card::{Headline, Made};
use crate::{icon, win32};

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

/// Open the cards for `sources`, their right edge on `anchor`'s, from its top down. The
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
    let y = anchor.top.max(work.top);
    let h = column_height(sources.len(), scale).min(work.bottom - y);
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
        is_movable: false,
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
                cx.defer(move |_| {
                    win32::move_window(HWND(raw as *mut _), x, y, w, h, true);
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
                if this.wait == wait && !this.hovered {
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
        cx.spawn(async move |_, _: &mut AsyncApp| {
            win32::resize_window(HWND(raw as *mut _), rect.right - rect.left, h);
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

    fn card(&self, index: usize, card: &Card, cx: &mut Context<Self>) -> Stateful<gpui::Div> {
        let done = matches!(card.status, Status::Done { .. });
        let picture = match (done, &card.sharp, &card.soft) {
            (true, Some(image), _) | (false, _, Some(image)) => img(image.clone())
                .absolute()
                .size_full()
                .rounded(px(RADIUS))
                .object_fit(ObjectFit::Fill)
                .into_any_element(),
            _ => div().into_any_element(),
        };
        let shade = linear_gradient(
            180.0,
            linear_color_stop(rgba(0x00000000), 0.35),
            linear_color_stop(rgba(0x000000b8), 1.0),
        );
        let opened = match &card.status {
            Status::Done { output, .. } => output.clone(),
            _ => card.source.clone(),
        };
        let left = if done {
            circle(("dismiss", index), glyph('\u{e8bb}', 10.0))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.dismiss(index, cx)))
        } else {
            let stop = div().size(px(10.0)).rounded(px(2.0)).bg(rgb(0x1c1c1e));
            circle(("stop", index), stop.into_any_element()).on_click(
                cx.listener(|this, _: &ClickEvent, _, _| {
                    this.cancel.store(true, Ordering::Relaxed)
                }),
            )
        };
        let right = circle(("open", index), glyph('\u{e8b9}', 13.0)).on_click(move |_, _, _| {
            let path = opened.clone();
            std::thread::spawn(move || win32::open_path(&path));
        });
        div()
            .id(index)
            .relative()
            .flex_none()
            .w(px(CARD_W))
            .h(px(CARD_H))
            .rounded(px(RADIUS))
            .bg(rgb(0x1c1c1e))
            .child(picture)
            .when(!done, |s| {
                s.child(
                    div()
                        .absolute()
                        .size_full()
                        .rounded(px(RADIUS))
                        .bg(rgba(0x00000033)),
                )
            })
            .child(div().absolute().size_full().rounded(px(RADIUS)).bg(shade))
            .child(left.absolute().top(px(10.0)).left(px(10.0)))
            .child(right.absolute().top(px(10.0)).right(px(10.0)))
            .child(
                div()
                    .absolute()
                    .left(px(14.0))
                    .right(px(14.0))
                    .bottom(px(12.0))
                    .child(self.caption(&card.status)),
            )
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(RADIUS))
                    .border_1()
                    .border_color(rgba(0xffffff2e)),
            )
    }

    fn caption(&self, status: &Status) -> AnyElement {
        let working = |label: String, progress: f32| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(label),
                )
                .child(
                    div().h(px(4.0)).rounded_full().bg(rgba(0xffffff40)).child(
                        div()
                            .h_full()
                            .w(px((CARD_W - 28.0) * progress.clamp(0.0, 1.0)))
                            .rounded_full()
                            .bg(rgb(0xffffff)),
                    ),
                )
                .into_any_element()
        };
        match status {
            Status::Waiting => working("Waiting".into(), 0.0),
            Status::Working { label, progress } => working(
                label
                    .clone()
                    .unwrap_or_else(|| format!("{}…", self.action.label())),
                *progress,
            ),
            Status::Done { made, .. } => {
                let headline = match made.headline() {
                    Headline::Sizes(before, after) => div()
                        .flex()
                        .gap(px(7.0))
                        .child(div().text_color(rgb(0xff6961)).child(before))
                        .child(div().text_color(rgba(0xffffffd9)).child("→"))
                        .child(div().text_color(rgb(0xa9a8ff)).child(after)),
                    Headline::Text(text) => div().child(text),
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .font_family(MONO)
                    .font_weight(FontWeight::BOLD)
                    .child(headline.text_size(px(18.0)).line_height(px(22.0)))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .line_height(px(15.0))
                            .truncate()
                            .child(made.detail()),
                    )
                    .into_any_element()
            }
        }
    }
}

/// A white round button on a card's corner.
fn circle(id: impl Into<gpui::ElementId>, content: AnyElement) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .size(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(rgba(0xf5f5f7eb))
        .text_color(rgb(0x1c1c1e))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(0xffffff)))
        .child(content)
}

fn glyph(c: char, size: f32) -> AnyElement {
    div()
        .font_family(ICONS)
        .text_size(px(size))
        .child(c.to_string())
        .into_any_element()
}

impl Render for Cards {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cards: Vec<_> = (self.cards.iter().enumerate())
            .filter(|(_, card)| !card.gone)
            .map(|(i, card)| self.card(i, card, cx))
            .collect();
        let column = div()
            .id("cards")
            .size_full()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .overflow_y_scroll()
            .text_color(rgb(0xffffff))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.hover(*hovered, cx)))
            .children(cards);
        if self.leaving {
            column
                .with_animation("leave", Animation::new(FADE), |s, t| s.opacity(1.0 - t))
                .into_any_element()
        } else {
            column.into_any_element()
        }
    }
}
