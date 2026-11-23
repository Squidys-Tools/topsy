//! How an activity looks in the notch: an icon, its name, and on the right what it's at. Two
//! share the notch side by side, unless a headset's card has it to itself. A finished download
//! shows itself in Explorer when clicked.

use std::time::{Duration, Instant};

use gpui::{AnyElement, ClickEvent, Context, Div, div, prelude::*, px, rgb, rgba};

use super::{State, card};
use crate::Island;
use crate::frame::PEEK;
use crate::hud::{GREEN, RED, cell};
use crate::icons::{self, Icon};
use model::activity::{Activity, Key};
use model::headset::spin;
use model::headset::{Form, Power};
use model::pomodoro::Phase;
use model::transform::size;

const ONE: f32 = 280.0;
const TWO: f32 = 440.0;
pub(super) const DIM: u32 = 0xffffff99;
/// The spin's redraws, about 60 a second, for its moment only.
const FRAME_TIME: Duration = Duration::from_millis(16);
const ORANGE: u32 = 0xff9f0a;

impl State {
    /// The peek's size while it shows activities.
    pub(crate) fn size(&self) -> Option<(f32, f32)> {
        if let Some((_, Activity::Connected { power, .. })) = self.list.card() {
            return Some(card::size(*power));
        }
        match self.list.shown().count() {
            0 => None,
            1 => Some((ONE, PEEK.1)),
            _ => Some((TWO, PEEK.1)),
        }
    }

    /// The activities in the dropped notch, if any are on. A peek opened to look shows them too.
    pub(crate) fn peek(&self, cx: &mut Context<Island>) -> Option<Div> {
        if let Some((_, Activity::Connected { name, form, power })) = self.list.card() {
            let since = self.card_since.get().unwrap_or_else(Instant::now);
            self.card_since.set(Some(since));
            let (frame, turning) = spin::frame_at(since.elapsed());
            if turning {
                self.spinning.book(FRAME_TIME, cx, |_, cx| cx.notify());
            }
            let art = self.art.frames(*form).and_then(|f| f.get(frame).cloned());
            return Some(card::card(name, *form, *power, art));
        }
        let (w, h) = self.size()?;
        Some(
            div()
                .w(px(w))
                .h(px(h))
                .flex_shrink_0()
                .px(px(14.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .text_xs()
                .children(self.list.shown().enumerate().flat_map(|(i, (key, a))| {
                    let divider =
                        (i > 0).then(|| div().w(px(1.0)).h(px(14.0)).bg(rgba(0xffffff2e)));
                    divider
                        .map(IntoElement::into_any_element)
                        .into_iter()
                        .chain([clickable(row(a), key, a, cx)])
                })),
        )
    }
}

fn clickable(row: Div, key: &Key, activity: &Activity, cx: &mut Context<Island>) -> AnyElement {
    let (Key::Download(id), Activity::Downloaded { path, .. }) = (key, activity) else {
        return row.into_any_element();
    };
    let (key, path) = (key.clone(), path.clone());
    row.id(("download", *id as usize))
        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            this.update_activities(cx, |activity| {
                Some(activity.show_download(&key, path.clone()))
            });
        }))
        .into_any_element()
}

fn row(activity: &Activity) -> Div {
    let (glyph, color, name, trailing) = match activity {
        Activity::Downloading { name, bytes } => (
            Icon::Download,
            rgb(0xffffff),
            name.as_str(),
            div().text_color(rgba(DIM)).child(size(*bytes)),
        ),
        Activity::Downloaded { name, .. } => (
            Icon::Tick,
            rgb(GREEN),
            name.as_str(),
            div().text_color(rgba(DIM)).child("Done"),
        ),
        Activity::Connected { name, form, power } => (
            glyph(*form),
            rgb(0xffffff),
            name.as_str(),
            match power.and_then(Power::lowest) {
                Some(p) => div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .child(div().text_color(rgba(DIM)).child(format!("{p}%")))
                    .child(cell(p, if p <= 20 { RED } else { GREEN })),
                None => div().text_color(rgba(DIM)).child("Connected"),
            },
        ),
        Activity::Disconnected { name, form } => (
            glyph(*form),
            rgba(DIM),
            name.as_str(),
            div().text_color(rgba(DIM)).child("Disconnected"),
        ),
        Activity::PhaseOver { ended } => {
            let (name, next) = match ended {
                Phase::Focus => ("Focus done", "Take a break"),
                Phase::Break => ("Break over", "Back to focus"),
            };
            (
                Icon::Timer,
                rgb(ORANGE),
                name,
                div().text_color(rgba(DIM)).child(next),
            )
        }
    };
    div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(icons::icon(glyph, 16.0, color).flex_shrink_0())
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .child(name.to_owned()),
        )
        .child(trailing.flex_shrink_0())
}

pub(super) fn glyph(form: Form) -> Icon {
    match form {
        Form::StemBuds => Icon::StemBuds,
        Form::Buds => Icon::Buds,
        Form::OverEar => Icon::Headphones,
        Form::Speaker => Icon::Speaker,
    }
}
