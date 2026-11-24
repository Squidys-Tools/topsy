//! Now Playing on the island: the player at the top of a tray opened to look (art, title and
//! artist, the progress bar, the buttons), laid out after Droppy's, and, while something plays,
//! the peek's hint (art and title). Pulled out of the tray, the player floats in a window of
//! its own ([`floating`]). The rules are in `model::media`, the system session in `session`.

mod art;
mod bars;
pub(crate) mod floating;
mod parts;
pub(crate) mod session;

use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;

use gpui::{
    App, ClickEvent, Context, Div, Entity, FontWeight, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, RenderImage, ScrollHandle, Stateful, div, point, prelude::*, px,
    rgb, rgba,
};
use windows::Win32::Foundation::HWND;

use crate::booking::Booking;
use crate::frame::Glass;
use crate::icons::{self, Icon};
use crate::now_playing::art::ArtSizes;
use crate::now_playing::bars::Bars;
use crate::now_playing::parts::{DIM, GLYPH, art, control, play_pause_glyph, timeline};
use crate::now_playing::session::{self as media, Button};
use crate::{Island, icon, log};
use model::marquee;
use model::media::{Card, Event, NowPlaying, Progress};

/// The art's side in the player; the peek shows it smaller.
const ART: f32 = 48.0;
const PEEK_ART: f32 = 20.0;
const PAD: f32 = 16.0;
/// The player's height, padding included: what the tray grows by while it shows.
pub const HEIGHT: f32 = 14.0 + ART + 12.0 + 12.0 + 10.0 + BUTTON.1 + 10.0;
const BUTTON: (f32, f32) = (48.0, 36.0);
/// The app's name beside the buttons, at most as wide as the room left of them.
const SOURCE_WIDTH: f32 = 96.0;
/// How far a press on the player travels before it pulls the player out of the notch.
const PULL: f32 = 10.0;

/// What the system's media session is playing, and its art.
#[derive(Default)]
pub struct Media {
    pub now: NowPlaying,
    art: Option<Arc<RenderImage>>,
    /// The bars' colour, from the art.
    accent: Option<[u8; 3]>,
    /// The progress bar's next second.
    ticking: Booking,
    bars: Bars,
    /// The bars' next frame.
    metering: Booking,
    /// The player's title line, scrolled when the title is too long for it.
    title: ScrollHandle,
    /// When this track's title was first shown, and whether its next scroll step is booked.
    title_shown: Cell<Option<Instant>>,
    scrolling: Booking,
    /// Where the left button went down on the player: moved far enough, it pulls it out.
    pull: Option<Point<Pixels>>,
    /// The floating player; none without its window.
    floating: Option<floating::Handle>,
}

/// What of the island is on screen, as the island decides it: Now Playing books redraws only
/// for what is.
#[derive(Clone, Copy)]
pub(crate) struct Shown {
    /// The island is not hidden.
    pub(crate) open: bool,
    pub(crate) tray: bool,
    /// The open tray shows the player.
    pub(crate) player: bool,
    pub(crate) peek: bool,
}

/// What the island must do in turn for a change Now Playing made.
pub(crate) enum Effect {
    /// A button took the left button: no chip is being pressed.
    Pressed,
    /// The session changed: an open tray springs to its sections' size.
    Refit,
    /// The player left the tray to float: the tray folds.
    PulledOut,
}

/// Start watching the system's media session, unless `TOPSY_NO_MEDIA` is set, and make the
/// floating player's window, glazed with `glass`, handing the foreground back to `previous`.
pub fn start(cx: &mut App, island: &Entity<Island>, glass: Glass, black: bool, previous: HWND) {
    if crate::config::get().no_media {
        return;
    }
    let scale = island.read(cx).scale;
    let sizes = ArtSizes {
        art: (ART * scale).round() as u32,
        cover: (floating::COVER * scale).round() as u32,
    };
    let player = floating::open(cx, glass, black, previous);
    island.update(cx, |island, _| island.media.floating = player);
    crate::app::forward(cx, island, media::spawn(sizes), |island, update, cx| {
        island.update_media(cx, |media, shown, cx| media.apply(update, shown, cx));
    });
}

impl Media {
    fn apply(
        &mut self,
        update: media::Update,
        shown: Shown,
        cx: &mut Context<Island>,
    ) -> Option<Effect> {
        log(&format!("media {:?}", update.event));
        if let Some(floating) = &self.floating {
            let update = update.clone();
            (floating.player).update(cx, |player, cx| player.apply(update, cx));
        }
        let track = matches!(update.event, Event::Track(_) | Event::Gone);
        // Only the open tray shows the position; apps report it every few seconds.
        let drawn = match update.event {
            Event::Timeline(_) => shown.player,
            _ => shown.open,
        };
        let mut changed = self.now.apply(update.event);
        if track {
            self.title_shown.set(None);
            changed |= self.art.is_some() || update.art.is_some();
            self.accent = update.art.as_ref().and_then(|a| a.accent);
            let art = update.art.map(|a| a.image);
            if let Some(old) = std::mem::replace(&mut self.art, art) {
                icon::forget(vec![old], cx);
            }
        }
        // What isn't drawn now is drawn from the new state when it next is.
        if changed && drawn {
            cx.notify();
        }
        Some(Effect::Refit)
    }

    /// Show the floating player, or put it away if it is out: the tray player's button and the
    /// tray icon's menu. Whether it is out now.
    pub(crate) fn toggle_floating(&mut self, cx: &mut Context<Island>) -> bool {
        let Some(floating) = &self.floating else {
            return false;
        };
        (floating.player).update(cx, |player, cx| player.toggle(cx));
        floating.is_out()
    }

    /// The *Black notch* setting reaches the floating player's glass too.
    pub(crate) fn set_black(&mut self, black: bool, cx: &mut Context<Island>) {
        if let Some(floating) = &self.floating {
            (floating.player).update(cx, |player, cx| player.set_black(black, cx));
        }
    }

    /// The player: art, title and artist; the progress bar; the buttons. A press on it that
    /// moves pulls it out of the notch.
    pub(crate) fn player(
        &self,
        shown: Shown,
        cx: &mut Context<Island>,
    ) -> Option<impl IntoElement + use<>> {
        let card = self.now.card()?;
        let Card {
            title,
            artist,
            playing,
            controls,
            source,
        } = card;
        let buttons = [
            (Button::Previous, Icon::Previous, controls.previous),
            (
                Button::PlayPause,
                play_pause_glyph(playing),
                controls.play_pause,
            ),
            (Button::Next, Icon::Next, controls.next),
        ]
        .map(|(button, glyph, enabled)| {
            let side = if button == Button::PlayPause {
                30.0
            } else {
                26.0
            };
            pressing(control(button, glyph, enabled, BUTTON, side), enabled, cx)
        });
        let progress = self.now.progress(Instant::now());
        if playing && let Some(p) = progress.filter(|p| p.elapsed < p.length) {
            self.tick(p, cx);
        }
        self.scroll_title(cx);
        let header = div()
            .h(px(ART))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.0))
            .children(art(self.art.as_ref(), ART, 10.0))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("title")
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .whitespace_nowrap()
                            .overflow_x_scroll()
                            .track_scroll(&self.title)
                            // GPUI scrolls as far as its children's boxes reach, and a block
                            // child is only as wide as the line: this one keeps the text's width.
                            .flex()
                            .child(div().flex_none().child(title.to_owned())),
                    )
                    .children(artist.map(|a| {
                        div()
                            .text_size(px(14.0))
                            .text_color(rgba(DIM))
                            .truncate()
                            .child(a.to_owned())
                    })),
            )
            .child(self.bars(22.0, 4.0, shown, cx));
        // Not read from the player's entity: reading it here would redraw the island whenever
        // the player draws.
        let floating = self.floating.as_ref().map(floating::Handle::is_out);
        Some(
            div()
                .id("media-player")
                .w_full()
                .h(px(HEIGHT))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .px(px(PAD))
                .pt(px(14.0))
                .child(header)
                .child(timeline(progress).mt(px(12.0)))
                .child(
                    div()
                        .relative()
                        .mt(px(10.0))
                        .flex()
                        .flex_row()
                        .justify_center()
                        .gap(px(14.0))
                        .children(buttons)
                        .children(source.map(|app| switch(app, cx)))
                        .children(floating.map(|out| pop_out(out, cx))),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|island, event: &MouseDownEvent, _window, cx| {
                        island.update_media(cx, |media, _, _| {
                            media.pull = Some(event.position);
                            None
                        });
                    }),
                )
                .on_mouse_move(cx.listener(|island, event: &MouseMoveEvent, _window, cx| {
                    island.update_media(cx, |media, _, cx| media.pulled(event, cx));
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|island, _: &MouseUpEvent, _window, _cx| {
                        island.media.pull = None;
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|island, _: &MouseUpEvent, _window, _cx| {
                        island.media.pull = None;
                    }),
                ),
        )
    }

    /// The left button is held on the player and has moved: far enough, the player comes out
    /// of the notch under the pointer and goes on with the drag.
    fn pulled(&mut self, event: &MouseMoveEvent, cx: &mut Context<Island>) -> Option<Effect> {
        if !event.dragging() {
            self.pull = None;
            return None;
        }
        let start = self.pull?;
        let (dx, dy) = (
            f32::from(event.position.x - start.x),
            f32::from(event.position.y - start.y),
        );
        if dx * dx + dy * dy < PULL * PULL {
            return None;
        }
        self.pull = None;
        let player = self.floating.as_ref()?.player.clone();
        log("media pulled out");
        player.update(cx, |player, cx| player.pull_out(cx));
        Some(Effect::PulledOut)
    }

    /// Redraw when the elapsed time next reads a new second, while the player is drawn. Nothing
    /// is booked otherwise, so a closed island costs nothing however long the song.
    fn tick(&self, progress: Progress, cx: &mut Context<Island>) {
        self.ticking
            .book(progress.until_next_second(), cx, |island, cx| {
                island.update_media(cx, |_, shown, cx| {
                    if shown.tray {
                        cx.notify();
                    }
                    None
                });
            });
    }

    /// Scroll a title too long for its line to where it is now, and book the next step. The
    /// overflow is the last layout's, so a new title starts moving a frame after it is drawn.
    fn scroll_title(&self, cx: &mut Context<Island>) {
        let now = Instant::now();
        let fresh = self.title_shown.get().is_none();
        let shown = *self.title_shown.get().get_or_insert(now);
        self.title_shown.set(Some(shown));
        let overflow = f32::from(self.title.max_offset().width);
        let next = match marquee::at(overflow, now - shown) {
            Some((x, next)) => {
                self.title.set_offset(point(px(-x), px(0.0)));
                next
            }
            None if fresh => marquee::FRAME,
            None => return,
        };
        self.scrolling.book(next, cx, |island, cx| {
            island.update_media(cx, |media, shown, cx| {
                if shown.player {
                    cx.notify();
                } else {
                    // Shown again, it starts from the beginning.
                    media.title_shown.set(None);
                }
                None
            });
        });
    }

    /// The peek's hint while something plays: the title with its art, and how many files
    /// (`count`) are on the shelf.
    pub(crate) fn peek_hint(
        &self,
        count: usize,
        shown: Shown,
        cx: &mut Context<Island>,
    ) -> Option<Div> {
        let title = self.now.hint()?;
        let hint = div()
            .w(px(crate::frame::PEEK.0))
            .flex_shrink_0()
            .px(px(14.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .text_xs()
            .children(art(self.art.as_ref(), PEEK_ART, 5.0))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .child(title.to_owned()),
            )
            .children((count > 0).then(|| div().text_color(rgba(DIM)).child(count.to_string())))
            .child(self.bars(14.0, 3.0, shown, cx));
        Some(hint)
    }

    /// Whether the level bars are on screen and moving: something plays and the peek or the
    /// player shows it.
    fn bars_live(&self, shown: Shown) -> bool {
        self.now.hint().is_some() && (shown.peek || shown.player)
    }

    /// The level bars, `height` tall, each `width` wide. While they are live each frame books
    /// the next; once they aren't, the meter is let go and nothing more is booked.
    fn bars(
        &self,
        height: f32,
        width: f32,
        shown: Shown,
        cx: &mut Context<Island>,
    ) -> impl IntoElement + use<> {
        if self.bars_live(shown) {
            self.metering.book(bars::FRAME, cx, |island, cx| {
                island.update_media(cx, |media, shown, cx| {
                    // A pause leaves the player on screen: its bars settle to dots.
                    if media.bars.frame(media.bars_live(shown)) && shown.open {
                        cx.notify();
                    }
                    None
                });
            });
        }
        self.bars.draw(height, width, self.accent)
    }
}

/// A media button in the tray: pressing it is not a press on a chip, nor the start of a pull.
fn pressing(button: Stateful<Div>, enabled: bool, cx: &mut Context<Island>) -> Stateful<Div> {
    if !enabled {
        return button;
    }
    button.on_mouse_down(
        MouseButton::Left,
        cx.listener(|island, _: &MouseDownEvent, _window, cx| {
            island.update_media(cx, |_, _, _| Some(Effect::Pressed));
            cx.stop_propagation();
        }),
    )
}

/// The app playing, beside the buttons while another app has a session too: a click shows the
/// next one.
fn switch(app: &str, cx: &mut Context<Island>) -> impl IntoElement + use<> {
    let button = div()
        .id("media-source")
        .absolute()
        .left_0()
        .top_0()
        .h(px(BUTTON.1))
        .max_w(px(SOURCE_WIDTH))
        .px(px(8.0))
        .rounded(px(10.0))
        .flex()
        .items_center()
        .text_size(px(12.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgba(DIM))
        .cursor_pointer()
        .hover(|style| style.bg(rgba(0xffffff14)).text_color(rgb(GLYPH)))
        .on_click(|_: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            log("media switch");
            media::switch();
        })
        .child(
            div()
                .max_w(px(SOURCE_WIDTH - 16.0))
                // Not `truncate`: GPUI keeps a no-wrap text's first measure, so it would never cut.
                .line_clamp(1)
                .text_ellipsis()
                .child(app.to_owned()),
        );
    pressing(button, true, cx)
}

/// At the right of the buttons: float the player, or, while it floats, put it away.
fn pop_out(out: bool, cx: &mut Context<Island>) -> impl IntoElement + use<> {
    let glyph = if out { rgb(GLYPH) } else { rgba(DIM) };
    let button = div()
        .id("media-float")
        .absolute()
        .right_0()
        .top_0()
        .size(px(BUTTON.1))
        .rounded(px(10.0))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.bg(rgba(0xffffff14)))
        .child(icons::icon(Icon::Open, 18.0, glyph))
        .on_click(cx.listener(|island, _: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            log("media float button");
            // The player has left the tray: it folds out of the way.
            island.update_media(cx, |media, _, cx| {
                media.toggle_floating(cx).then_some(Effect::PulledOut)
            });
        }));
    pressing(button, true, cx)
}
