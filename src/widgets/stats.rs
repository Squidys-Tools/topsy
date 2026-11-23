//! The system stats card, after Droppy's: CPU on the left and memory on the right, each a ring
//! gauge, its percentage and a rolling graph. It samples once a second only while it is drawn;
//! the rules are in `model::stats`, the readings in `win32::sysinfo`.

use std::time::Instant;

use gpui::{
    AnyElement, Bounds, Context, FontWeight, MouseButton, Pixels, Point, Window, canvas, div,
    linear_color_stop, linear_gradient, point, prelude::*, px, rgba,
};

use crate::booking::Booking;
use crate::icons::{self, Icon};
use crate::log::LogErr;
use crate::win32::sysinfo;
use crate::{Island, ring};
use model::stats::{CAPACITY, History, Stats, percent};

pub const WIDTH: f32 = 208.0;
const HALF: f32 = (WIDTH - GUTTER) / 2.0;
const GUTTER: f32 = 12.0;
const RING: f32 = 36.0;
const GAP: f32 = 6.0;
const RING_LINE: f32 = 3.0;
const GRAPH: f32 = 24.0;
const CPU: u32 = 0x8e8cff;
const RAM: u32 = 0x5ee6c8;
const DIM: u32 = 0xffffff80;

#[derive(Default)]
pub struct State {
    stats: Stats,
    /// The next sample.
    booked: Booking,
}

impl State {
    pub(super) fn card(&self, visible: bool, cx: &mut Context<Island>) -> AnyElement {
        self.book(visible, cx);
        let stats = &self.stats;
        let memory = stats.memory();
        let cpu = gauge(Side::Cpu, stats.cpu(), None);
        let ram = gauge(
            Side::Ram,
            memory.map(|memory| memory.load()),
            memory.map(|memory| memory.gauge()),
        );
        let graphs = div()
            .flex()
            .flex_row()
            .justify_between()
            .child(graph(
                stats.cpu_history(),
                stats.cpu_history().ceiling(),
                CPU,
            ))
            .child(graph(stats.memory_history(), 1.0, RAM));
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_between()
            .pt(px(2.0))
            .pb(px(4.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .child(cpu)
                    .child(ram),
            )
            .child(graphs)
            .into_any_element()
    }

    /// Book the next sample while the card is drawn; a folded tray books nothing.
    fn book(&self, visible: bool, cx: &mut Context<Island>) {
        if !visible {
            return;
        }
        let after = self.stats.next_sample(Instant::now());
        self.booked.book(after, cx, |island, cx| {
            island.update_widgets(cx, |widgets, visible, cx| {
                widgets.stats.sample(visible, cx);
                None
            });
        });
    }

    fn sample(&mut self, visible: bool, cx: &mut Context<Island>) {
        if !visible {
            return;
        }
        let times = sysinfo::cpu_times().log_err();
        let memory = sysinfo::memory().log_err();
        self.stats.sample(times, memory, Instant::now());
        cx.notify();
    }
}

/// Which half: CPU reads left to right, memory mirrors it.
#[derive(Clone, Copy)]
enum Side {
    Cpu,
    Ram,
}

/// The ring under its label with the percentage beside it, on the card's outer edge.
fn gauge(side: Side, load: Option<f32>, detail: Option<String>) -> impl IntoElement {
    let (label, glyph, colour) = match side {
        Side::Cpu => ("CPU", Icon::Cpu, CPU),
        Side::Ram => ("RAM", Icon::Ram, RAM),
    };
    let figure = div()
        .flex()
        .flex_row()
        .items_baseline()
        .child(
            div()
                .text_size(px(22.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(load.map_or_else(|| "–".to_string(), percent)),
        )
        .child(
            div()
                .ml(px(1.0))
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgba(DIM))
                .child("%"),
        );
    let reading = div()
        .flex()
        .flex_col()
        .child(figure)
        .children(detail.map(|text| {
            div()
                .mt(px(-3.0))
                .text_size(px(10.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgba(DIM))
                .whitespace_nowrap()
                .child(text)
        }));
    let gauge = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(3.0))
        .child(
            div()
                .text_size(px(11.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgba(colour << 8 | 0xff))
                .child(label),
        )
        .child(ring(load, glyph, colour));
    let reading = reading.pb(px(2.0));
    let row = div().flex().flex_row().items_end().gap(px(GAP));
    match side {
        Side::Cpu => row.child(gauge).child(reading.items_start()),
        Side::Ram => row.child(reading.items_end()).child(gauge),
    }
}

/// The gauge: a faint track and an arc clockwise from the top, the icon inside.
fn ring(load: Option<f32>, glyph: Icon, colour: u32) -> impl IntoElement {
    let fraction = load.unwrap_or(0.0);
    div()
        .relative()
        .size(px(RING))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    ring::paint(bounds, (RING, RING_LINE), fraction, colour, window)
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .child(icons::icon(glyph, 16.0, rgba(colour << 8 | 0xff)))
}

/// The history as a line with a fading fill under it, newest at the right edge.
fn graph(history: &History, ceiling: f32, colour: u32) -> impl IntoElement {
    let values: Vec<f32> = history.values().collect();
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| paint_graph(bounds, &values, ceiling, colour, window),
    )
    .w(px(HALF))
    .h(px(GRAPH))
    .flex_shrink_0()
}

fn paint_graph(
    bounds: Bounds<Pixels>,
    values: &[f32],
    ceiling: f32,
    colour: u32,
    window: &mut Window,
) {
    if values.len() < 2 {
        return;
    }
    let line = 1.5;
    let origin = bounds.origin;
    let width: f32 = bounds.size.width.into();
    let height: f32 = bounds.size.height.into();
    let step = width / (CAPACITY - 1) as f32;
    let start = width - step * (values.len() - 1) as f32;
    let points: Vec<Point<Pixels>> = values
        .iter()
        .enumerate()
        .map(|(i, value)| {
            let level = (value / ceiling).clamp(0.0, 1.0);
            let y = line + (height - 2.0 * line) * (1.0 - level);
            point(origin.x + px(start + step * i as f32), origin.y + px(y))
        })
        .collect();
    let bottom = origin.y + px(height);
    let mut area = gpui::PathBuilder::fill();
    area.move_to(point(points[0].x, bottom));
    for corner in &points {
        area.line_to(*corner);
    }
    area.line_to(point(points[points.len() - 1].x, bottom));
    area.close();
    if let Ok(path) = area.build() {
        let fade = linear_gradient(
            180.0,
            linear_color_stop(rgba(colour << 8 | 0x40), 0.0),
            linear_color_stop(rgba(colour << 8), 1.0),
        );
        window.paint_path(path, fade);
    }
    let mut stroke = gpui::PathBuilder::stroke(px(line));
    stroke.move_to(points[0]);
    for corner in &points[1..] {
        stroke.line_to(*corner);
    }
    if let Ok(path) = stroke.build() {
        window.paint_path(path, rgba(colour << 8 | 0xff));
    }
}
