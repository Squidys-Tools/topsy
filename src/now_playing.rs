//! Now Playing on the island: a row in the tray (art, title, artist, buttons) and, while
//! something plays, the peek's hint (art and title). The rules are in `model::media`, the
//! system session in `media`.

use std::sync::Arc;

use gpui::{
    App, Bounds, ClickEvent, Context, Entity, MouseButton, MouseDownEvent, ObjectFit, Pixels,
    RenderImage, Window, canvas, div, fill, img, point, prelude::*, px, rgb, rgba, size,
};

use crate::media::{self, Button};
use crate::model::media::{Card, Event};
use crate::{Island, Mode, icon, log};

/// The art's side in the tray's row; the peek shows it smaller.
const ART: f32 = 40.0;
const PEEK_ART: f32 = 20.0;
/// The tray's row and the gap above the shelf: what the tray grows by while there is one.
pub const ROW: f32 = ART + 6.0;

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
        let had_card = self.media.card().is_some();
        let track = matches!(update.event, Event::Track(_) | Event::Gone);
        let mut changed = self.media.apply(update.event);
        if track {
            changed |= self.art.is_some() || update.art.is_some();
            if let Some(old) = std::mem::replace(&mut self.art, update.art) {
                icon::forget(vec![old], cx);
            }
        }
        if self.media.card().is_some() != had_card && self.mode == Mode::Tray {
            // The tray grows or shrinks by the row.
            self.height.set_target(self.size_for(Mode::Tray).1);
        }
        // Hidden draws nothing; the next reveal renders the new state anyway.
        if changed && self.mode != Mode::Hidden {
            cx.notify();
        }
    }

    /// The tray's row, when something is playing or paused.
    pub(crate) fn media_row(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let card = self.media.card()?;
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
        Some(
            div()
                .w_full()
                .h(px(ART))
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .children(art(self.art.as_ref(), ART, 8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .child(div().text_sm().truncate().child(title.to_owned()))
                        .children(artist.map(|a| {
                            div()
                                .text_xs()
                                .text_color(rgba(0xffffffb3))
                                .truncate()
                                .child(a.to_owned())
                        })),
                )
                .child(div().flex().flex_row().gap(px(2.0)).children(buttons)),
        )
    }

    /// What the peek says: the playing title with its art, or what is on the shelf.
    pub(crate) fn peek_content(&self) -> impl IntoElement + use<> {
        let count = self.shelf.len();
        let Some(title) = self.media.hint() else {
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
            .children(art(self.art.as_ref(), PEEK_ART, 4.0))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .child(title.to_owned()),
            )
            .children(
                (count > 0).then(|| div().text_color(rgba(0xffffffb3)).child(count.to_string())),
            )
    }
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

/// A round button drawn with paths (no icon font to depend on). Pressing it asks the session's
/// app from the media thread; the glyph changes when the app reports back.
fn control(
    button: Button,
    glyph: Glyph,
    enabled: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement + use<> {
    let base = div()
        .id(("media", button as usize))
        .size(px(28.0))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| paint(glyph, bounds, window),
            )
            .size(px(12.0)),
        );
    if !enabled {
        return base.opacity(0.35);
    }
    base.cursor_pointer()
        .hover(|style| style.bg(rgba(0xffffff1a)))
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
    let white = rgb(0xffffff);
    let bar = |x: f32, w: f32, window: &mut Window| {
        window.paint_quad(fill(Bounds::new(p(x, 1.0), size(px(w), px(10.0))), white));
    };
    let triangle = |tip: f32, base: f32, window: &mut Window| {
        let mut path = gpui::PathBuilder::fill();
        path.move_to(p(base, 1.0));
        path.line_to(p(tip, 6.0));
        path.line_to(p(base, 11.0));
        path.close();
        if let Ok(path) = path.build() {
            window.paint_path(path, white);
        }
    };
    match glyph {
        Glyph::Play => triangle(11.0, 2.0, window),
        Glyph::Pause => {
            bar(2.0, 3.0, window);
            bar(7.0, 3.0, window);
        }
        Glyph::Previous => {
            bar(1.0, 2.0, window);
            triangle(3.0, 11.0, window);
        }
        Glyph::Next => {
            triangle(9.0, 1.0, window);
            bar(9.0, 2.0, window);
        }
    }
}
