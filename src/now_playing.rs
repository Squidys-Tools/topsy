//! Now Playing on the island: the player at the top of a tray opened to look (art, title and
//! artist, the progress bar, the buttons), laid out after Droppy's, and, while something plays,
//! the peek's hint (art and title). The rules are in `model::media`, the system session in
//! `media`.

use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;

use gpui::{
    App, Bounds, ClickEvent, Context, Entity, FontWeight, MouseButton, MouseDownEvent, ObjectFit,
    Pixels, RenderImage, Window, canvas, div, fill, img, point, prelude::*, px, relative, rgb,
    rgba,
};

use crate::media::{self, Button};
use crate::model::media::{Card, Event, NowPlaying, Progress, clock};
use crate::{Island, Mode, icon, log};

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
    /// A redraw is booked for the progress bar's next second.
    ticking: Cell<bool>,
}

/// Start watching the system's media session, unless `TOPSY_NO_MEDIA` is set.
pub fn start(cx: &mut App, island: &Entity<Island>) {
    if crate::config::get().no_media {
        return;
    }
    let art_size = (ART * island.read(cx).scale).round() as u32;
    crate::app::forward(cx, island, media::spawn(art_size), Island::on_media);
}

impl Island {
    pub(crate) fn on_media(&mut self, update: media::Update, cx: &mut Context<Self>) {
        log(&format!("media {:?}", update.event));
        let track = matches!(update.event, Event::Track(_) | Event::Gone);
        // Only the open tray shows the position; apps report it every few seconds.
        let shown = match update.event {
            Event::Timeline(_) => self.mode == Mode::Tray && self.sections().media,
            _ => self.mode != Mode::Hidden,
        };
        let mut changed = self.media.now.apply(update.event);
        if track {
            changed |= self.media.art.is_some() || update.art.is_some();
            if let Some(old) = std::mem::replace(&mut self.media.art, update.art) {
                icon::forget(vec![old], cx);
            }
        }
        self.fit_tray();
        // What isn't drawn now is drawn from the new state when it next is.
        if changed && shown {
            cx.notify();
        }
    }

    /// The player: art, title and artist; the progress bar; the buttons.
    pub(crate) fn player(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let card = self.media.now.card()?;
        let Card {
            title,
            artist,
            playing,
            controls,
        } = card;
        let buttons = [
            (Button::Previous, Glyph::Previous, controls.previous),
            (
                Button::PlayPause,
                if playing { Glyph::Pause } else { Glyph::Play },
                controls.play_pause,
            ),
            (Button::Next, Glyph::Next, controls.next),
        ]
        .map(|(button, glyph, enabled)| control(button, glyph, enabled, cx));
        let progress = self.media.now.progress(Instant::now());
        if playing && let Some(p) = progress.filter(|p| p.elapsed < p.length) {
            self.tick(p, cx);
        }
        let header = div()
            .h(px(ART))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.0))
            .children(art(self.media.art.as_ref(), ART, 10.0))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(title.to_owned()),
                    )
                    .children(artist.map(|a| {
                        div()
                            .text_size(px(14.0))
                            .text_color(rgba(DIM))
                            .truncate()
                            .child(a.to_owned())
                    })),
            );
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
    fn tick(&self, progress: Progress, cx: &mut Context<Self>) {
        if self.media.ticking.replace(true) {
            return;
        }
        let after = progress.until_next_second();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            this.update(cx, |this, cx| {
                this.media.ticking.set(false);
                if this.mode == Mode::Tray {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// What the peek says: the playing title with its art, or what is on the shelf.
    pub(crate) fn peek_content(&self) -> impl IntoElement + use<> {
        let count = self.shelf.len();
        let Some(title) = self.media.now.hint() else {
            let label = match count {
                0 => "topsy".to_string(),
                n => format!("{n} on shelf"),
            };
            return div().text_xs().child(label);
        };
        div()
            .w(px(crate::PEEK.0))
            .flex_shrink_0()
            .px(px(14.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .text_xs()
            .children(art(self.media.art.as_ref(), PEEK_ART, 5.0))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .child(title.to_owned()),
            )
            .children((count > 0).then(|| div().text_color(rgba(DIM)).child(count.to_string())))
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
            .object_fit(ObjectFit::Cover)
    })
}

#[derive(Clone, Copy)]
enum Glyph {
    Previous,
    Play,
    Pause,
    Next,
}

impl Glyph {
    /// Play and pause are the larger hero button, as in Droppy.
    fn size(self) -> (f32, f32) {
        match self {
            Glyph::Play | Glyph::Pause => (18.0, 22.0),
            Glyph::Previous | Glyph::Next => (26.0, 15.0),
        }
    }
}

/// A button drawn with paths (no icon font to depend on). Pressing it asks the session's app
/// from the media thread; the glyph changes when the app reports back.
fn control(
    button: Button,
    glyph: Glyph,
    enabled: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement + use<> {
    let (w, h) = glyph.size();
    let base = div()
        .id(("media", button as usize))
        .w(px(BUTTON.0))
        .h(px(BUTTON.1))
        .rounded(px(10.0))
        .flex()
        .items_center()
        .justify_center()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| paint(glyph, bounds, window),
            )
            .w(px(w))
            .h(px(h)),
        );
    if !enabled {
        return base.opacity(0.3);
    }
    base.cursor_pointer()
        .hover(|style| style.bg(rgba(0xffffff14)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                this.press = None;
                cx.stop_propagation();
            }),
        )
        .on_click(cx.listener(move |_, _: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            log(&format!("media press {button:?}"));
            media::press(button);
        }))
}

fn paint(glyph: Glyph, bounds: Bounds<Pixels>, window: &mut Window) {
    let o = bounds.origin;
    let p = |x: f32, y: f32| point(o.x + px(x), o.y + px(y));
    let (w, h) = glyph.size();
    let color = rgb(GLYPH);
    // A triangle from a flat side at `base` to a point at `tip`, the glyph's full height.
    let triangle = |base: f32, tip: f32, window: &mut Window| {
        let mut path = gpui::PathBuilder::fill();
        path.move_to(p(base, 0.0));
        path.line_to(p(tip, h / 2.0));
        path.line_to(p(base, h));
        path.close();
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    };
    match glyph {
        Glyph::Play => triangle(1.0, w, window),
        Glyph::Pause => {
            for x in [1.0, w - 7.0] {
                let bar = Bounds::new(p(x, 0.0), gpui::size(px(6.0), px(h)));
                window.paint_quad(fill(bar, color).corner_radii(px(2.0)));
            }
        }
        Glyph::Next => {
            triangle(0.0, w / 2.0, window);
            triangle(w / 2.0, w, window);
        }
        Glyph::Previous => {
            triangle(w / 2.0, 0.0, window);
            triangle(w, w / 2.0, window);
        }
    }
}
