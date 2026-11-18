//! The system stats card, after Droppy's: CPU on the left and memory on the right, each a ring
//! gauge, its percentage and a rolling graph. It samples once a second only while it is drawn;
//! the rules are in `model::stats`, the readings in `win32::sysinfo`.

use std::cell::Cell;
use std::f32::consts::TAU;
use std::time::Instant;

use gpui::{
    AnyElement, Bounds, Context, FontWeight, MouseButton, Pixels, Point, Window, canvas, div, fill,
    linear_color_stop, linear_gradient, point, prelude::*, px, rgba, size,
};

use crate::Island;
use crate::icons::{self, Icon};
use crate::log::LogErr;
use crate::win32::sysinfo;
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
    /// The next sample is booked.
    booked: Cell<bool>,
}

/// Which half: CPU reads left to right, memory mirrors it.
#[derive(Clone, Copy)]
enum Side {
    Cpu,
    Ram,
}

impl Island {
    pub(crate) fn stats_card(&self, cx: &mut Context<Self>) -> AnyElement {
        self.book_stats(cx);
        let stats = &self.widgets.stats.stats;
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
    fn book_stats(&self, cx: &mut Context<Self>) {
        if !self.widgets_visible() || self.widgets.stats.booked.replace(true) {
            return;
        }
        let after = self.widgets.stats.stats.next_sample(Instant::now());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            this.update(cx, |this, cx| {
                this.widgets.stats.booked.set(false);
                if !this.widgets_visible() {
                    return;
                }
                let times = sysinfo::cpu_times().log_err();
                let memory = sysinfo::memory().log_err();
                this.widgets
                    .stats
                    .stats
                    .sample(times, memory, Instant::now());
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
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
                move |bounds, (), window, _| paint_ring(bounds, fraction, colour, window),
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .child(icons::icon(glyph, 16.0, rgba(colour << 8 | 0xff)))
}

fn paint_ring(bounds: Bounds<Pixels>, fraction: f32, colour: u32, window: &mut Window) {
    let radius = (RING - RING_LINE) / 2.0;
    let centre = bounds.center();
    let at = |angle: f32| -> Point<Pixels> {
        point(
            centre.x + px(radius * angle.sin()),
            centre.y - px(radius * angle.cos()),
        )
    };
    let radii = point(px(radius), px(radius));
    let mut track = gpui::PathBuilder::stroke(px(RING_LINE));
    track.move_to(at(0.0));
    track.arc_to(radii, px(0.0), false, true, at(TAU / 2.0));
    track.arc_to(radii, px(0.0), false, true, at(0.0));
    if let Ok(path) = track.build() {
        window.paint_path(path, rgba(colour << 8 | 0x33));
    }
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction <= 0.0 {
        return;
    }
    let solid = rgba(colour << 8 | 0xff);
    let end = fraction * TAU;
    let mut arc = gpui::PathBuilder::stroke(px(RING_LINE));
    arc.move_to(at(0.0));
    if fraction >= 0.999 {
        arc.arc_to(radii, px(0.0), false, true, at(TAU / 2.0));
        arc.arc_to(radii, px(0.0), false, true, at(0.0));
    } else {
        arc.arc_to(radii, px(0.0), fraction > 0.5, true, at(end));
    }
    if let Ok(path) = arc.build() {
        window.paint_path(path, solid);
    }
    // PathBuilder's caps are square; dots at both ends round them off.
    for angle in [0.0, end] {
        let dot = at(angle);
        let half = RING_LINE / 2.0;
        let cap = Bounds::new(
            point(dot.x - px(half), dot.y - px(half)),
            size(px(RING_LINE), px(RING_LINE)),
        );
        window.paint_quad(fill(cap, solid).corner_radii(px(half)));
    }
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
