//! The card a headset connecting opens with, after the iPhone's: its picture on the left, its
//! name, and its batteries on the right. One battery is a large ring beside the name; earbuds
//! that report each bud and the case get a small ring for each.

use std::sync::Arc;

use gpui::{Div, RenderImage, canvas, div, img, prelude::*, px, rgb, rgba};

use super::view::{DIM, glyph};
use crate::hud::{GREEN, RED};
use crate::{icons, ring};
use model::headset::{Form, Power};

/// Under 70 px tall: the island takes anything taller and wider than 260 px for the tray.
const HEIGHT: f32 = 64.0;
const ART: f32 = 56.0;
const BIG: (f32, f32) = (42.0, 4.0);
const SMALL: (f32, f32) = (28.0, 3.0);
/// A battery at or below this is drawn red.
const LOW: u8 = 20;

/// Wider for a ring per bud and the case, so the name keeps its room.
pub(super) fn size(power: Option<Power>) -> (f32, f32) {
    match power {
        Some(Power::Split { .. }) => (420.0, HEIGHT),
        Some(Power::Single(_)) | None => (340.0, HEIGHT),
    }
}

/// `art` is the turntable's frame to show; the flat glyph stands in without one.
pub(super) fn card(
    name: &str,
    form: Form,
    power: Option<Power>,
    art: Option<Arc<RenderImage>>,
) -> Div {
    let (width, height) = size(power);
    div()
        .w(px(width))
        .h(px(height))
        .flex_shrink_0()
        .px(px(18.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(14.0))
        .child(
            div()
                .size(px(ART))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(match art {
                    Some(frame) => img(frame).size(px(ART)).into_any_element(),
                    None => icons::icon(glyph(form), 40.0, rgb(0xffffff)).into_any_element(),
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(div().text_sm().truncate().child(name.to_owned()))
                .child(div().text_xs().text_color(rgba(DIM)).child("Connected")),
        )
        .children(power.map(batteries))
}

fn batteries(power: Power) -> Div {
    match power {
        Power::Single(percent) => gauge(percent, BIG, format!("{percent}%"), None),
        Power::Split { left, right, case } => div()
            .flex_shrink_0()
            .flex()
            .flex_row()
            .gap(px(10.0))
            .children(
                [(left, "L"), (right, "R"), (case, "Case")]
                    .into_iter()
                    .filter_map(|(percent, label)| {
                        Some(gauge(percent?, SMALL, percent?.to_string(), Some(label)))
                    }),
            ),
    }
}

/// A ring at `percent` with `text` inside, and `label` under it.
fn gauge(percent: u8, (diameter, line): (f32, f32), text: String, label: Option<&str>) -> Div {
    let colour = if percent <= LOW { RED } else { GREEN };
    let fraction = f32::from(percent.min(100)) / 100.0;
    let ring = div()
        .relative()
        .size(px(diameter))
        .flex()
        .items_center()
        .justify_center()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    ring::paint(bounds, (diameter, line), fraction, colour, window)
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .child(
            div()
                .text_size(px(if label.is_some() { 10.0 } else { 12.0 }))
                .child(text),
        );
    div()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(2.0))
        .child(ring)
        .children(label.map(|label| {
            div()
                .text_size(px(10.0))
                .text_color(rgba(DIM))
                .child(label.to_owned())
        }))
}
