//! How an activity looks in the notch: an icon, its name, and on the right what it's at. Two
//! share the notch side by side. A finished download shows itself in Explorer when clicked.

use gpui::{AnyElement, ClickEvent, Context, Div, div, prelude::*, px, rgb, rgba};

use crate::Island;
use crate::frame::PEEK;
use crate::hud::{GREEN, RED, cell};
use crate::icons::{self, Icon};
use crate::model::activity::{Activity, Key};
use crate::model::pomodoro::Phase;
use crate::model::transform::size;

const ONE: f32 = 280.0;
const TWO: f32 = 440.0;
const DIM: u32 = 0xffffff99;
const ORANGE: u32 = 0xff9f0a;

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
    pub(crate) fn activity_peek(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (w, h) = self.activity_size()?;
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
                .children(
                    self.activity
                        .list
                        .shown()
                        .enumerate()
                        .flat_map(|(i, (key, a))| {
                            let divider =
                                (i > 0).then(|| div().w(px(1.0)).h(px(14.0)).bg(rgba(0xffffff2e)));
                            divider
                                .map(IntoElement::into_any_element)
                                .into_iter()
                                .chain([clickable(row(a), key, a, cx)])
                        }),
                ),
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
            this.show_download(&key, path.clone(), cx);
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
        Activity::Connected { name, battery } => (
            Icon::Headphones,
            rgb(0xffffff),
            name.as_str(),
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
