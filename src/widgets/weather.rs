//! The weather card, after Droppy's: the temperature where the machine is, today's high and low,
//! and the next hours, from Open-Meteo. It asks only while the row is on screen and a fetch is
//! due (`model::weather`); the last forecast stays for the next opening.

use std::cell::Cell;
use std::time::Instant;

use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, MouseButton, MouseDownEvent, div,
    linear_color_stop, linear_gradient, prelude::*, px, rgb, rgba,
};

use crate::icons::{self, Icon};
use crate::log::LogErr;
use crate::win32::http::{self, Response};
use crate::win32::location;
use crate::{Island, log};
use model::weather::{self, Condition, Forecast, HOST, Hour, LIMIT, Problem, Show, Unit, Weather};

pub const WIDTH: f32 = 184.0;
const RADIUS: f32 = 16.0;
const PAD: f32 = 10.0;
const TEXT: u32 = 0xffffff;
const DIM: u32 = 0xffffffb3;
const DAY: (u32, u32) = (0x4a90d9, 0x6aa8e8);
const NIGHT: (u32, u32) = (0x1c2a4a, 0x2c3e66);

#[derive(Default)]
pub struct State {
    weather: Weather,
    /// A fetch is on its way; at most one at a time.
    fetching: Cell<bool>,
}

impl Island {
    pub(crate) fn weather_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.widgets.weather;
        if self.widgets_visible() && !state.fetching.get() && state.weather.due(Instant::now()) {
            self.fetch_weather(cx);
        }
        let show = state.weather.show();
        let is_day = match show {
            Show::Forecast(forecast) => forecast.is_day,
            Show::Loading | Show::LocationOff | Show::Unavailable => true,
        };
        let (top, bottom) = if is_day { DAY } else { NIGHT };
        let card = div()
            .id("weather")
            .size_full()
            .rounded(px(RADIUS))
            .bg(linear_gradient(
                180.0,
                linear_color_stop(rgb(top), 0.0),
                linear_color_stop(rgb(bottom), 1.0),
            ))
            .p(px(PAD))
            .flex()
            .flex_col()
            .text_color(rgb(TEXT))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                    this.press = None;
                    cx.stop_propagation();
                }),
            );
        match show {
            Show::Forecast(forecast) => card.child(header(forecast)).child(hours(&forecast.hours)),
            Show::Loading => card.child(place()).child(note("Loading…", None)),
            Show::LocationOff => card
                .cursor_pointer()
                .child(place())
                .child(note("Location is off", Some("Click to open settings")))
                .on_click(cx.listener(|_, _: &ClickEvent, _window, cx| {
                    cx.stop_propagation();
                    std::thread::spawn(location::open_settings);
                })),
            Show::Unavailable => card
                .child(place())
                .child(note("No weather", Some("Trying again soon"))),
        }
        .into_any_element()
    }

    /// Locate, ask Open-Meteo and read the answer on a thread of its own: each step blocks for
    /// seconds at worst.
    fn fetch_weather(&self, cx: &mut Context<Self>) {
        self.widgets.weather.fetching.set(true);
        let (sender, receiver) = async_channel::bounded(1);
        let spawned = std::thread::Builder::new()
            .name("topsy-weather".into())
            .spawn(move || {
                sender.send_blocking(fetch()).log_err();
            });
        if let Err(err) = spawned {
            eprintln!("topsy: no weather thread: {err}");
        }
        cx.spawn(async move |this, cx| {
            let result = receiver.recv().await.unwrap_or(Err(Problem::Failed));
            this.update(cx, |this, cx| {
                log(&format!(
                    "weather {:?}",
                    result.as_ref().map(|f| f.temperature)
                ));
                let state = &mut this.widgets.weather;
                state.fetching.set(false);
                state.weather.record(Instant::now(), result);
                if this.widgets_visible() {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

fn fetch() -> Result<Forecast, Problem> {
    let (latitude, longitude) = location::locate().map_err(|err| match err {
        location::Error::Off => Problem::NoLocation,
        location::Error::Unavailable => Problem::Failed,
    })?;
    let unit = if location::us_units() {
        Unit::Fahrenheit
    } else {
        Unit::Celsius
    };
    match http::get(HOST, &weather::path(latitude, longitude, unit), LIMIT) {
        Response::Body(body) => weather::parse(&body).map_err(|err| {
            eprintln!("topsy: unreadable forecast: {err}");
            Problem::Failed
        }),
        Response::Redirect(_) | Response::Missing | Response::Failed => Err(Problem::Failed),
    }
}

fn condition_icon(condition: Condition, is_day: bool) -> Icon {
    match (condition, is_day) {
        (Condition::Clear, true) => Icon::Sun,
        (Condition::Clear, false) => Icon::Moon,
        (Condition::PartlyCloudy, true) => Icon::SunCloud,
        (Condition::PartlyCloudy, false) => Icon::MoonCloud,
        (Condition::Cloudy, _) => Icon::Cloud,
        (Condition::Fog, _) => Icon::Fog,
        (Condition::Drizzle, _) => Icon::Drizzle,
        (Condition::Rain, _) => Icon::Rain,
        (Condition::Snow, _) => Icon::Snow,
        (Condition::Thunder, _) => Icon::Thunder,
    }
}

fn small(size: f32, color: impl Into<Hsla>) -> Div {
    div()
        .text_size(px(size))
        .line_height(px(size + 2.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .whitespace_nowrap()
}

/// Open-Meteo has no place names, so the card says where in the only way it can.
fn place() -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(3.0))
        .child(small(11.0, rgb(TEXT)).child("My location"))
        .child(icons::icon(Icon::Location, 10.0, rgb(TEXT)))
}

fn note(title: &'static str, detail: Option<&'static str>) -> Div {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(2.0))
        .child(small(15.0, rgb(TEXT)).child(title))
        .children(detail.map(|detail| small(10.0, rgba(DIM)).child(detail)))
}

/// The place and the temperature on the left; the condition, high and low on the right.
fn header(forecast: &Forecast) -> Div {
    let right = div()
        .flex()
        .flex_col()
        .items_end()
        .child(icons::icon(
            condition_icon(forecast.condition, forecast.is_day),
            14.0,
            rgb(TEXT),
        ))
        .child(
            small(10.0, rgb(TEXT))
                .mt(px(3.0))
                .child(forecast.condition.label(forecast.is_day)),
        )
        .child(small(10.0, rgb(TEXT)).child(format!("H:{}° L:{}°", forecast.high, forecast.low)));
    let left = div().flex().flex_col().child(place()).child(
        div()
            .text_size(px(28.0))
            .line_height(px(28.0))
            .font_weight(FontWeight::LIGHT)
            .child(format!("{}°", forecast.temperature)),
    );
    div()
        .flex()
        .flex_row()
        .justify_between()
        .child(left)
        .child(right)
}

fn hours(hours: &[Hour]) -> Div {
    let hour = |hour: &Hour| {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(3.0))
            .child(small(9.0, rgba(DIM)).child(hour.hour.to_string()))
            .child(icons::icon(
                condition_icon(hour.condition, hour.is_day),
                13.0,
                rgb(TEXT),
            ))
            .child(small(10.0, rgb(TEXT)).child(format!("{}°", hour.temperature)))
    };
    div()
        .mt_auto()
        .flex()
        .flex_row()
        .justify_between()
        .px(px(2.0))
        .children(hours.iter().map(hour))
}
