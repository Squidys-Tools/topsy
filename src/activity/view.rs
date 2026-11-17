//! How an activity looks in the notch: an icon, its name, and on the right what it's at. Two
//! share the notch side by side.

use gpui::{Div, div, prelude::*, px, rgb, rgba};

use crate::frame::PEEK;
use crate::hud::{GREEN, RED, cell};
use crate::icons::{self, Icon};
use crate::model::activity::Activity;
use crate::model::transform::size;
use crate::{Island, Mode};

const ONE: f32 = 280.0;
const TWO: f32 = 440.0;
const DIM: u32 = 0xffffff99;

impl Island {
    /// The peek's size while it shows activities.
    pub(crate) fn activity_size(&self) -> Option<(f32, f32)> {
        match self.activity.list.shown().count() {
            0 => None,
            1 => Some((ONE, PEEK.1)),
            _ => Some((TWO, PEEK.1)),
        }
    }

    /// The activities in the dropped notch, if any are on. A peek opened to look shows them too.
    pub(crate) fn activity_peek(&self) -> Option<Div> {
        let (w, h) = self.activity_size()?;
        debug_assert!(self.mode != Mode::Tray);
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
                .children(self.activity.list.shown().enumerate().flat_map(|(i, a)| {
                    let divider =
                        (i > 0).then(|| div().w(px(1.0)).h(px(14.0)).bg(rgba(0xffffff2e)));
                    divider
                        .map(IntoElement::into_any_element)
                        .into_iter()
                        .chain([row(a).into_any_element()])
                })),
        )
    }
}

fn row(activity: &Activity) -> Div {
    let (glyph, color, name, trailing) = match activity {
        Activity::Downloading { name, bytes } => (
            Icon::Download,
            rgb(0xffffff),
            name,
            div().text_color(rgba(DIM)).child(size(*bytes)),
        ),
        Activity::Downloaded { name } => (
            Icon::Tick,
            rgb(GREEN),
            name,
            div().text_color(rgba(DIM)).child("Done"),
        ),
        Activity::Connected { name, battery } => (
            Icon::Headphones,
            rgb(0xffffff),
            name,
            match battery {
                Some(p) => div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .child(div().text_color(rgba(DIM)).child(format!("{p}%")))
                    .child(cell(*p, if *p <= 20 { RED } else { GREEN })),
                None => div().text_color(rgba(DIM)).child("Connected"),
            },
        ),
        Activity::Disconnected { name } => (
            Icon::Headphones,
            rgba(DIM),
            name,
            div().text_color(rgba(DIM)).child("Disconnected"),
        ),
    };
    div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(icons::icon(glyph, 16.0, color).flex_shrink_0())
        .child(div().flex_1().min_w(px(0.0)).truncate().child(name.clone()))
        .child(trailing.flex_shrink_0())
}
