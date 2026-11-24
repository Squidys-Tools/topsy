//! How the floating player draws at each size: Mini (the art, the title and play/pause), Player
//! (the notch's player) and Expanded (the cover over the player). Everything but the buttons is
//! its caption, so it is moved by any of it.

use std::time::Instant;

use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, ScrollWheelEvent, Stateful, Window,
    WindowControlArea, div, prelude::*, px, rgb, rgba,
};

use super::Player;
use crate::icons::{self, Icon};
use crate::log;
use crate::now_playing::bars;
use crate::now_playing::parts::{DIM, art, control, play_pause_glyph, timeline};
use crate::now_playing::session::{self as media, Button};
use crate::win32;
use model::media::{Card, Controls};
use model::pager::Step;
use model::player::Size;

/// The expanded player's cover, square.
pub(crate) const COVER: f32 = 288.0;
const PAD: f32 = 16.0;
const MINI: (f32, f32) = (288.0, 56.0);
const MINI_ART: f32 = 40.0;
const ART: f32 = 48.0;
const BUTTON: (f32, f32) = (48.0, 36.0);
const SMALL_BUTTON: f32 = 28.0;
/// Below the art: the title, the progress line and the buttons, with the gaps between.
const BELOW: f32 = 12.0 + 12.0 + 10.0 + BUTTON.1 + 14.0;
const TITLES: f32 = 40.0;
/// An empty art's fill.
const WELL: u32 = 0xffffff14;

/// The player's size at `size`, in logical pixels.
pub(super) fn dims(size: Size) -> (f32, f32) {
    match size {
        Size::Mini => MINI,
        Size::Player => (320.0, 14.0 + ART + BELOW),
        Size::Expanded => (COVER + 2.0 * PAD, PAD + COVER + 14.0 + TITLES + BELOW),
    }
}

/// Where its art's centre is, from its top left: what the pointer holds as it is pulled out.
pub(super) fn grip(size: Size) -> (f32, f32) {
    match size {
        Size::Mini => (8.0 + MINI_ART / 2.0, MINI.1 / 2.0),
        Size::Player => (PAD + ART / 2.0, 14.0 + ART / 2.0),
        Size::Expanded => (PAD + COVER / 2.0, PAD + COVER / 2.0),
    }
}

impl Render for Player {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let dt = (self.last_frame)
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(1.0 / 120.0);
        let moving = self.width.step(dt) | self.height.step(dt);
        self.last_frame = moving.then_some(now);
        if moving {
            window.request_animation_frame();
        }
        let (width, height) = (self.width.value, self.height.value);
        self.frame
            .sync(width, height, 0.0, window.scale_factor(), false, cx);
        // After the move `sync` queued: the move loop must start from where the window now is.
        if std::mem::take(&mut self.grab)
            && let Some(hwnd) = self.hwnd
        {
            cx.spawn(async move |_, _| win32::grab::move_with_pointer(hwnd))
                .detach();
        }
        let viewport = window.viewport_size();
        let content = if self.open {
            self.card(cx).into_any_element()
        } else {
            div().into_any_element()
        };
        div()
            .id("floating-player")
            .relative()
            .w(viewport.width)
            .h(viewport.height)
            .text_color(rgb(0xffffff))
            .children(self.frame.draw(viewport, self.height.target, content, None))
            .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                this.hovered = *hovered;
                if this.size != Size::Mini && this.now.hint().is_some() {
                    cx.notify();
                }
            }))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, _cx| {
                this.swiped(event);
            }))
    }
}

impl Player {
    /// A sideways swipe skips to the next track or back to the previous one, once per swipe.
    fn swiped(&mut self, event: &ScrollWheelEvent) {
        let delta = event.delta.pixel_delta(px(16.0));
        let Some(step) = (self.swipe).scroll(delta.x.into(), delta.y.into(), Instant::now()) else {
            return;
        };
        let controls = self
            .now
            .card()
            .map(|card| card.controls)
            .unwrap_or_default();
        let (button, enabled) = match step {
            Step::On => (Button::Next, controls.next),
            Step::Back => (Button::Previous, controls.previous),
        };
        log(&format!("player swipe {button:?}"));
        if enabled {
            media::press(button);
        }
    }

    /// The player at its full size, so it is revealed (not squashed) while the window springs.
    fn card(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let card = self.now.card();
        let playing = card.is_some_and(|card| card.playing);
        let (w, h) = dims(self.size);
        let body = match self.size {
            Size::Mini => self.mini(card, cx).into_any_element(),
            Size::Player | Size::Expanded => self.full(card, cx).into_any_element(),
        };
        // The clock ticks while it shows; Mini has none, so it costs nothing as it plays.
        if playing
            && self.size != Size::Mini
            && let Some(p) = self.now.progress(Instant::now())
            && p.elapsed < p.length
        {
            self.ticking.book(p.until_next_second(), cx, |this, cx| {
                if this.open {
                    cx.notify();
                }
            });
        }
        if self.bars_live() {
            self.metering
                .book(bars::FRAME, cx, |this, cx| this.meter_frame(cx));
        }
        div()
            .id("player-card")
            .w(px(w))
            .h(px(h))
            .flex_shrink_0()
            .relative()
            .window_control_area(WindowControlArea::Drag)
            .child(body)
    }

    fn mini(&self, card: Option<Card<'_>>, cx: &mut Context<Self>) -> Div {
        let playing = card.is_some_and(|card| card.playing);
        let controls = card.map(|card| card.controls).unwrap_or_default();
        div()
            .size_full()
            .pl(px(8.0))
            .pr(px(6.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .child(self.picture(MINI_ART, 10.0, false))
            .child(titles(card, 13.0, 12.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(button(control(
                        Button::PlayPause,
                        play_pause_glyph(playing),
                        controls.play_pause,
                        (36.0, 36.0),
                        22.0,
                    )))
                    .child(grow(cx))
                    .child(close(cx)),
            )
    }

    /// Player and Expanded: the art and titles (side by side, or the cover above them), the
    /// progress line, then the buttons with the window's own beside them.
    fn full(&self, card: Option<Card<'_>>, cx: &mut Context<Self>) -> Div {
        let playing = card.is_some_and(|card| card.playing);
        let controls = card.map(|card| card.controls).unwrap_or_default();
        let bars = card.map(|_| self.bars.draw(22.0, 4.0, self.accent));
        let top = match self.size {
            Size::Expanded => div()
                .flex()
                .flex_col()
                .child(self.picture(COVER, 16.0, true))
                .child(
                    div()
                        .mt(px(14.0))
                        .h(px(TITLES))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(titles(card, 16.0, 13.0))
                        .children(bars),
                ),
            Size::Mini | Size::Player => div()
                .h(px(ART))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .child(self.picture(ART, 10.0, false))
                .child(titles(card, 15.0, 14.0))
                .children(bars),
        };
        let progress = self.now.progress(Instant::now());
        div()
            .size_full()
            .flex()
            .flex_col()
            .px(px(PAD))
            .pt(px(if self.size == Size::Expanded {
                PAD
            } else {
                14.0
            }))
            .child(top)
            .child(timeline(progress).mt(px(12.0)))
            .child(
                div()
                    .relative()
                    .mt(px(10.0))
                    .flex()
                    .flex_row()
                    .justify_center()
                    .gap(px(14.0))
                    .children(transport(playing, controls))
                    .child(
                        div()
                            .absolute()
                            .left(px(-8.0))
                            .top(px((BUTTON.1 - SMALL_BUTTON) / 2.0))
                            .child(close(cx)),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(px(-8.0))
                            .top(px((BUTTON.1 - SMALL_BUTTON) / 2.0))
                            .flex()
                            .flex_row()
                            .children(self.size.shrink().map(|_| shrink(cx)))
                            .children(self.size.grow().map(|_| grow(cx))),
                    ),
            )
    }

    /// The art, or the cover when `cover`; an empty well with nothing playing or no art.
    fn picture(&self, side: f32, radius: f32, cover: bool) -> AnyElement {
        let image = if cover { &self.cover } else { &self.art };
        match art(image.as_ref(), side, radius) {
            Some(art) => art.into_any_element(),
            None => div()
                .size(px(side))
                .flex_shrink_0()
                .rounded(px(radius))
                .bg(rgba(WELL))
                .flex()
                .items_center()
                .justify_center()
                .child(icons::icon(Icon::Headphones, side * 0.42, rgba(DIM)))
                .into_any_element(),
        }
    }
}

fn close(cx: &mut Context<Player>) -> Stateful<Div> {
    small("player-close", Icon::Close).on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
        cx.stop_propagation();
        this.close(cx);
    }))
}

fn grow(cx: &mut Context<Player>) -> Stateful<Div> {
    resize_button("player-grow", Icon::Resize, Size::grow, cx)
}

fn shrink(cx: &mut Context<Player>) -> Stateful<Div> {
    resize_button("player-shrink", Icon::Shrink, Size::shrink, cx)
}

/// Grow or shrink the player a size, as `to` says.
fn resize_button(
    id: &'static str,
    glyph: Icon,
    to: fn(Size) -> Option<Size>,
    cx: &mut Context<Player>,
) -> Stateful<Div> {
    small(id, glyph).on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
        cx.stop_propagation();
        if let Some(size) = to(this.size) {
            this.resize(size, cx);
        }
    }))
}

/// The title over the artist, or "Nothing playing". Not `truncate`: GPUI keeps a no-wrap text's
/// first measure, so it would never cut.
fn titles(card: Option<Card<'_>>, title_size: f32, artist_size: f32) -> Div {
    let line = |text: String, size: f32| {
        div()
            .text_size(px(size))
            .line_clamp(1)
            .text_ellipsis()
            .child(text)
    };
    let (title, artist) = match card {
        Some(card) => (card.title.to_owned(), card.artist.map(str::to_owned)),
        None => ("Nothing playing".to_owned(), None),
    };
    let title = line(title, title_size).font_weight(FontWeight::SEMIBOLD);
    let title = if card.is_some() {
        title
    } else {
        title.text_color(rgba(DIM))
    };
    div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .child(title)
        .children(artist.map(|artist| line(artist, artist_size).text_color(rgba(DIM))))
}

/// Previous, play/pause and next.
fn transport(playing: bool, controls: Controls) -> [Stateful<Div>; 3] {
    [
        (Button::Previous, Icon::Previous, controls.previous, 26.0),
        (
            Button::PlayPause,
            play_pause_glyph(playing),
            controls.play_pause,
            30.0,
        ),
        (Button::Next, Icon::Next, controls.next, 26.0),
    ]
    .map(|(which, glyph, enabled, side)| button(control(which, glyph, enabled, BUTTON, side)))
}

/// A button on the caption: it takes its own clicks rather than moving the window.
fn button(element: Stateful<Div>) -> Stateful<Div> {
    element.occlude()
}

/// One of the window's own buttons, quieter than the media's.
fn small(id: &'static str, glyph: Icon) -> Stateful<Div> {
    button(
        div()
            .id(id)
            .size(px(SMALL_BUTTON))
            .flex_shrink_0()
            .rounded(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| style.bg(rgba(0xffffff14)))
            .child(icons::icon(glyph, 15.0, rgba(DIM))),
    )
}
