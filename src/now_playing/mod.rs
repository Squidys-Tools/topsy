//! Now Playing on the island: the player at the top of a tray opened to look (art, title and
//! artist, the progress bar, the buttons), laid out after Droppy's, and, while something plays,
//! the peek's hint (art and title). The rules are in `model::media`, the system session in
//! `media`.

pub(crate) mod session;

use std::cell::Cell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    App, Bounds, ClickEvent, Context, Div, Entity, FontWeight, MouseButton, MouseDownEvent,
    ObjectFit, RenderImage, ScrollHandle, canvas, div, fill, img, point, prelude::*, px, relative,
    rgb, rgba,
};

use crate::booking::Booking;
use crate::icons::{self, Icon};
use crate::now_playing::session::{self as media, Button};
use crate::win32::meter::Meter;
use crate::{Island, icon, log};
use model::levels::{self, BARS, Levels};
use model::marquee;
use model::media::{Card, Event, NowPlaying, Progress, clock};

/// The art's side in the player; the peek shows it smaller.
const ART: f32 = 48.0;
const PEEK_ART: f32 = 20.0;
const PAD: f32 = 16.0;
/// The player's height, padding included: what the tray grows by while it shows.
pub const HEIGHT: f32 = 14.0 + ART + 12.0 + TIMES + 10.0 + BUTTON.1 + 10.0;
const TIMES: f32 = 12.0;
const TIME_WIDTH: f32 = 48.0;
const BAR: f32 = 8.0;
const BUTTON: (f32, f32) = (48.0, 36.0);
const DIM: u32 = 0xffffff80;
const GLYPH: u32 = 0xebebeb;

/// What the system's media session is playing, and its art.
#[derive(Default)]
pub struct Media {
    pub now: NowPlaying,
    art: Option<Arc<RenderImage>>,
    /// The bars' colour, from the art.
    accent: Option<[u8; 3]>,
    /// The progress bar's next second.
    ticking: Booking,
    levels: Levels,
    /// Open only while the bars move, with when it was opened.
    meter: Option<(Meter, Instant)>,
    /// The bars' next frame.
    metering: Booking,
    /// The player's title line, scrolled when the title is too long for it.
    title: ScrollHandle,
    /// When this track's title was first shown, and whether its next scroll step is booked.
    title_shown: Cell<Option<Instant>>,
    scrolling: Booking,
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
}

/// Start watching the system's media session, unless `TOPSY_NO_MEDIA` is set.
pub fn start(cx: &mut App, island: &Entity<Island>) {
    if crate::config::get().no_media {
        return;
    }
    let art_size = (ART * island.read(cx).scale).round() as u32;
    crate::app::forward(cx, island, media::spawn(art_size), |island, update, cx| {
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

    /// The player: art, title and artist; the progress bar; the buttons.
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
        } = card;
        let buttons = [
            (Button::Previous, Icon::Previous, controls.previous),
            (
                Button::PlayPause,
                if playing { Icon::Pause } else { Icon::Play },
                controls.play_pause,
            ),
            (Button::Next, Icon::Next, controls.next),
        ]
        .map(|(button, glyph, enabled)| control(button, glyph, enabled, cx));
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
        Some(
            div()
                .w_full()
                .h(px(HEIGHT))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .px(px(PAD))
                .pt(px(14.0))
                .child(header)
                .child(timeline(progress))
                .child(
                    div()
                        .mt(px(10.0))
                        .flex()
                        .flex_row()
                        .justify_center()
                        .gap(px(14.0))
                        .children(buttons),
                ),
        )
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
            let frame = Duration::from_millis(levels::FRAME_MS);
            self.metering.book(frame, cx, |island, cx| {
                island.update_media(cx, |media, shown, cx| {
                    media.meter_frame(shown, cx);
                    None
                });
            });
        }
        let heights = self.levels.bars();
        let color = match self.accent {
            Some([r, g, b]) => rgb(u32::from_be_bytes([0, r, g, b])),
            None => rgb(GLYPH),
        };
        let gap = width * 0.75;
        canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let o = bounds.origin;
                for (i, h) in heights.iter().enumerate() {
                    let h = (height * h).max(width);
                    let x = i as f32 * (width + gap);
                    let bar = Bounds::new(
                        point(o.x + px(x), o.y + px((height - h) / 2.0)),
                        gpui::size(px(width), px(h)),
                    );
                    window.paint_quad(fill(bar, color).corner_radii(px(width / 2.0)));
                }
            },
        )
        .w(px(BARS as f32 * (width + gap) - gap))
        .h(px(height))
        .flex_shrink_0()
    }

    fn meter_frame(&mut self, shown: Shown, cx: &mut Context<Island>) {
        if !self.bars_live(shown) {
            self.meter = None;
            let moving = self.levels.bars() != [levels::REST; BARS];
            self.levels.rest();
            // A pause leaves the player on screen: settle its bars to dots.
            if moving && shown.open {
                cx.notify();
            }
            return;
        }
        // Reopened every second, so a change of output device (headphones in) is followed.
        let now = Instant::now();
        let stale = self
            .meter
            .as_ref()
            .is_none_or(|(_, opened)| now - *opened >= Duration::from_secs(1));
        if stale {
            self.meter = Some((Meter::open(), now));
        }
        let peak = self.meter.as_ref().map_or(0.0, |(m, _)| m.peak());
        self.levels.push(peak);
        cx.notify();
    }
}

/// Elapsed, the bar, and what is left. Without a length (a live stream) the bar stays empty
/// and the times blank, so the buttons don't jump.
fn timeline(progress: Option<Progress>) -> impl IntoElement {
    let time = |text: String| {
        div()
            .w(px(TIME_WIDTH))
            .flex_shrink_0()
            .text_size(px(12.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgba(DIM))
            .child(text)
    };
    let (elapsed, left, fraction) = match progress {
        Some(p) => (
            clock(p.elapsed),
            format!("−{}", clock(p.remaining())),
            p.fraction(),
        ),
        None => (String::new(), String::new(), 0.0),
    };
    div()
        .mt(px(12.0))
        .h(px(TIMES))
        .flex()
        .flex_row()
        .items_center()
        .child(time(elapsed))
        .child(
            div()
                .flex_1()
                .h(px(BAR))
                .rounded_full()
                .bg(rgba(0xffffff26))
                .child(
                    div()
                        .h_full()
                        .w(relative(fraction))
                        .rounded_full()
                        .bg(rgba(0xffffff99)),
                ),
        )
        .child(time(left).flex().justify_end())
}

fn art(art: Option<&Arc<RenderImage>>, side: f32, radius: f32) -> Option<impl IntoElement> {
    art.map(|art| {
        img(art.clone())
            .size(px(side))
            .flex_shrink_0()
            .rounded(px(radius))
            .object_fit(ObjectFit::Fill)
    })
}

/// A media button. Pressing it asks the session's app from the media thread; the icon changes
/// when the app reports back. Play and pause are the larger hero button, as in Droppy.
fn control(
    button: Button,
    glyph: Icon,
    enabled: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement + use<> {
    let side = if matches!(glyph, Icon::Play | Icon::Pause) {
        30.0
    } else {
        26.0
    };
    let base = div()
        .id(("media", button as usize))
        .w(px(BUTTON.0))
        .h(px(BUTTON.1))
        .rounded(px(10.0))
        .flex()
        .items_center()
        .justify_center()
        .child(icons::icon(glyph, side, rgb(GLYPH)));
    if !enabled {
        return base.opacity(0.3);
    }
    base.cursor_pointer()
        .hover(|style| style.bg(rgba(0xffffff14)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|island, _: &MouseDownEvent, _window, cx| {
                island.update_media(cx, |_, _, _| Some(Effect::Pressed));
                cx.stop_propagation();
            }),
        )
        .on_click(cx.listener(move |_, _: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            log(&format!("media press {button:?}"));
            media::press(button);
        }))
}
