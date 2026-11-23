//! The calendar card, after Droppy's: today and its ISO week over a month grid, with today in a
//! red circle. The chevrons browse other months; a click on today goes back to this month. The
//! date is read at render, so the card books no redraws of its own. The rules are in
//! `model::calendar`.

use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, MouseButton, div, prelude::*, px, rgb, rgba,
};

use super::{Card, Droplet, keep_press};
use crate::Island;
use crate::hud::RED;
use crate::icons::{self, Icon};
use crate::win32::clock;
use model::calendar::{Browse, Date, Weekday, YearMonth, week_from};

const WIDTH: f32 = 184.0;
const HEADER: f32 = 15.0;
const INITIALS: f32 = 11.0;
/// Six weeks fill what the header and the initials leave of the card.
const ROW: f32 = (super::CARD - HEADER - INITIALS) / 6.0;
const DAY_TEXT: f32 = 9.5;
const TODAY: f32 = 12.5;
const TEXT: u32 = 0xffffff;
const DIM: u32 = 0xffffff80;
const FAINT: u32 = 0xffffff4d;

#[derive(Default)]
pub struct State {
    browse: Browse,
}

impl Droplet for State {
    /// Show this month again.
    fn opened(&mut self, _cx: &mut Context<Island>) {
        self.browse = Browse::default();
    }

    fn card(&self) -> Option<&dyn Card> {
        Some(self)
    }
}

impl Card for State {
    fn width(&self) -> f32 {
        WIDTH
    }

    fn render(&self, _visible: bool, cx: &mut Context<Island>) -> AnyElement {
        let today = clock::today();
        let first = clock::first_weekday();
        let this_month = today.year_month();
        let shown = self.browse.shown(this_month);
        let week = week_from(first);
        div()
            .size_full()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, cx.listener(keep_press))
            .child(header(today, shown, cx))
            .child(row(INITIALS).children(week.map(|day| {
                cell(INITIALS)
                    .text_size(px(8.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgba(if day.is_weekend() { FAINT } else { DIM }))
                    .child(day.initial().to_string())
            })))
            .children(shown.grid(first).into_iter().map(|days| {
                row(ROW).children(days.iter().zip(week).map(|(day, weekday)| {
                    let today = (shown == this_month).then_some(today.day);
                    day_cell(*day, weekday, today)
                }))
            }))
            .into_any_element()
    }
}

/// "WED 18 W47" on the left, going back to this month when clicked; the shown month on the right
/// between chevrons.
fn header(today: Date, shown: YearMonth, cx: &mut Context<Island>) -> Div {
    let (_, week) = today.iso_week();
    let this_month = today.year_month();
    let label = if shown.year == today.year {
        shown.short().to_owned()
    } else {
        format!("{} {}", shown.short(), shown.year)
    };
    let away = shown != this_month;
    row(HEADER)
        .justify_between()
        .text_size(px(11.0))
        .font_weight(FontWeight::BOLD)
        .child(
            div()
                .id("calendar-today")
                .flex()
                .flex_row()
                .items_baseline()
                .gap(px(4.0))
                .when(away, |element| element.cursor_pointer())
                .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                    cx.stop_propagation();
                    this.update_droplet(cx, |calendar: &mut State, _, cx| {
                        if calendar.browse.is_away(clock::today().year_month()) {
                            calendar.browse = Browse::default();
                            cx.notify();
                        }
                        None
                    });
                }))
                .child(div().text_color(rgb(RED)).child(today.weekday().short()))
                .child(div().text_color(rgb(TEXT)).child(today.day.to_string()))
                .child(
                    div()
                        .text_size(px(9.5))
                        .text_color(rgba(DIM))
                        .child(format!("W{week}")),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .child(chevron(Icon::ChevronLeft, -1, cx))
                .child(
                    div()
                        .text_color(if away { rgb(TEXT) } else { rgb(RED) })
                        .child(label),
                )
                .child(chevron(Icon::ChevronRight, 1, cx)),
        )
}

fn chevron(glyph: Icon, months: i32, cx: &mut Context<Island>) -> impl IntoElement + use<> {
    div()
        .id(("calendar-step", (months + 1) as usize))
        .w(px(HEADER))
        .h(px(HEADER))
        .rounded(px(4.0))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.bg(rgba(0xffffff14)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            this.update_droplet(cx, |calendar: &mut State, _, cx| {
                calendar.browse.step(months, clock::today().year_month());
                cx.notify();
                None
            });
        }))
        .child(icons::icon(glyph, 10.0, rgba(DIM)))
}

fn day_cell(day: Option<u8>, weekday: Weekday, today: Option<u8>) -> Div {
    let Some(day) = day else {
        return cell(ROW);
    };
    if today == Some(day) {
        return cell(ROW).child(
            div()
                .size(px(TODAY))
                .rounded_full()
                .bg(rgb(RED))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(DAY_TEXT))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(TEXT))
                .child(day.to_string()),
        );
    }
    cell(ROW)
        .text_size(px(DAY_TEXT))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(if weekday.is_weekend() {
            rgba(DIM)
        } else {
            rgb(TEXT)
        })
        .child(day.to_string())
}

fn row(height: f32) -> Div {
    div()
        .w_full()
        .h(px(height))
        .flex_shrink_0()
        .flex()
        .flex_row()
        .items_center()
}

fn cell(height: f32) -> Div {
    div()
        .flex_1()
        .h(px(height))
        .flex()
        .items_center()
        .justify_center()
        .line_height(px(height))
}
