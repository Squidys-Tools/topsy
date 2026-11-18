//! The weather widget's rules: the Open-Meteo request, reading its answer into a forecast, what
//! the card shows, and when to ask again.

use std::time::{Duration, Instant};

use serde::Deserialize;

pub const HOST: &str = "api.open-meteo.com";
/// An answer is about 2 KB; anything near this is not one.
pub const LIMIT: usize = 64 * 1024;
/// Hours the card shows after the current one.
pub const HOURS: usize = 4;
const REFRESH: Duration = Duration::from_secs(30 * 60);
const RETRY: Duration = Duration::from_secs(5 * 60);
/// Location off is answered locally, and the user may have just turned it on.
const RETRY_NO_LOCATION: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Celsius,
    Fahrenheit,
}

/// The request's path for a position. Coordinates are rounded to about a kilometre: the
/// forecast is no different and the exact position never leaves the machine.
pub fn path(latitude: f64, longitude: f64, unit: Unit) -> String {
    let unit = match unit {
        Unit::Celsius => "",
        Unit::Fahrenheit => "&temperature_unit=fahrenheit",
    };
    format!(
        "/v1/forecast?latitude={latitude:.2}&longitude={longitude:.2}\
         &current=temperature_2m,weather_code,is_day\
         &hourly=temperature_2m,weather_code,is_day\
         &daily=temperature_2m_max,temperature_2m_min\
         &forecast_days=2&timezone=auto{unit}"
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Condition {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Thunder,
}

impl Condition {
    /// From a WMO weather code; one Open-Meteo doesn't document reads as cloudy.
    pub fn from_code(code: u8) -> Self {
        match code {
            0 | 1 => Self::Clear,
            2 => Self::PartlyCloudy,
            45 | 48 => Self::Fog,
            51..=57 => Self::Drizzle,
            61..=67 | 80..=82 => Self::Rain,
            71..=77 | 85 | 86 => Self::Snow,
            95..=99 => Self::Thunder,
            _ => Self::Cloudy,
        }
    }

    pub fn label(self, is_day: bool) -> &'static str {
        match self {
            Self::Clear if is_day => "Sunny",
            Self::Clear => "Clear",
            Self::PartlyCloudy => "Partly cloudy",
            Self::Cloudy => "Cloudy",
            Self::Fog => "Fog",
            Self::Drizzle => "Drizzle",
            Self::Rain => "Rain",
            Self::Snow => "Snow",
            Self::Thunder => "Thunderstorms",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hour {
    /// Local to the forecast's place, 0 to 23.
    pub hour: u8,
    pub temperature: i32,
    pub condition: Condition,
    pub is_day: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Forecast {
    pub temperature: i32,
    pub condition: Condition,
    pub is_day: bool,
    /// Today's.
    pub high: i32,
    pub low: i32,
    /// The next hours, strictly after the current one.
    pub hours: Vec<Hour>,
}

#[derive(Deserialize)]
struct Answer {
    current: Current,
    hourly: Hourly,
    daily: Daily,
}

#[derive(Deserialize)]
struct Current {
    time: String,
    temperature_2m: f64,
    weather_code: u8,
    is_day: u8,
}

#[derive(Deserialize)]
struct Hourly {
    time: Vec<String>,
    temperature_2m: Vec<f64>,
    weather_code: Vec<u8>,
    is_day: Vec<u8>,
}

#[derive(Deserialize)]
struct Daily {
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
}

/// Read Open-Meteo's answer. An answer missing a field, or with no day in it, is an error.
pub fn parse(body: &[u8]) -> Result<Forecast, String> {
    let answer: Answer = serde_json::from_slice(body).map_err(|err| err.to_string())?;
    let (Some(high), Some(low)) = (
        answer.daily.temperature_2m_max.first(),
        answer.daily.temperature_2m_min.first(),
    ) else {
        return Err("no daily forecast".into());
    };
    let current = answer.current;
    let now = hour_key(&current.time).ok_or("unreadable current time")?;
    let hourly = answer.hourly;
    let hours = hourly
        .time
        .iter()
        .zip(&hourly.temperature_2m)
        .zip(&hourly.weather_code)
        .zip(&hourly.is_day)
        .filter_map(|(((time, temperature), code), is_day)| {
            // ISO times sort as text, so a later hour on the next day still compares greater.
            let key = hour_key(time).filter(|key| *key > now)?;
            Some(Hour {
                hour: key[11..].parse().ok()?,
                temperature: degrees(*temperature),
                condition: Condition::from_code(*code),
                is_day: *is_day != 0,
            })
        })
        .take(HOURS)
        .collect();
    Ok(Forecast {
        temperature: degrees(current.temperature_2m),
        condition: Condition::from_code(current.weather_code),
        is_day: current.is_day != 0,
        high: degrees(*high),
        low: degrees(*low),
        hours,
    })
}

/// `2026-11-18T06:15` to its hour, `2026-11-18T06`.
fn hour_key(time: &str) -> Option<&str> {
    let key = time.get(..13)?;
    (key.as_bytes()[10] == b'T').then_some(key)
}

fn degrees(value: f64) -> i32 {
    value.round() as i32
}

/// Why an attempt brought no forecast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// Location is off for the machine or for desktop apps, or was refused.
    NoLocation,
    /// No position, or Open-Meteo couldn't be reached or made no sense.
    Failed,
}

/// What the card shows.
#[derive(Debug, PartialEq)]
pub enum Show<'a> {
    Forecast(&'a Forecast),
    Loading,
    LocationOff,
    Unavailable,
}

/// The last forecast, kept across openings, and how the last attempt went.
#[derive(Default)]
pub struct Weather {
    forecast: Option<Forecast>,
    last: Option<(Instant, Result<(), Problem>)>,
}

impl Weather {
    /// Whether to fetch now: never yet, or long enough after the last attempt for how it went.
    pub fn due(&self, now: Instant) -> bool {
        let Some((at, outcome)) = self.last else {
            return true;
        };
        let wait = match outcome {
            Ok(()) => REFRESH,
            Err(Problem::Failed) => RETRY,
            Err(Problem::NoLocation) => RETRY_NO_LOCATION,
        };
        now.saturating_duration_since(at) >= wait
    }

    pub fn record(&mut self, now: Instant, result: Result<Forecast, Problem>) {
        let outcome = result.map(|forecast| {
            self.forecast = Some(forecast);
        });
        self.last = Some((now, outcome));
    }

    /// The last forecast if there is one, even when the latest attempt failed.
    pub fn show(&self) -> Show<'_> {
        if let Some(forecast) = &self.forecast {
            return Show::Forecast(forecast);
        }
        match self.last {
            None | Some((_, Ok(()))) => Show::Loading,
            Some((_, Err(Problem::NoLocation))) => Show::LocationOff,
            Some((_, Err(Problem::Failed))) => Show::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real answer for Lisbon, trimmed to the hours around the current one.
    const LISBON: &str = r#"{"latitude":38.746044,"longitude":-9.175565,"generationtime_ms":0.13768672943115234,"utc_offset_seconds":0,"timezone":"Europe/Lisbon","timezone_abbreviation":"GMT","elevation":64.0,"current_units":{"time":"iso8601","interval":"seconds","temperature_2m":"°C","weather_code":"wmo code","is_day":""},"current":{"time":"2026-11-18T06:15","interval":900,"temperature_2m":19.4,"weather_code":3,"is_day":0},"hourly_units":{"time":"iso8601","temperature_2m":"°C","weather_code":"wmo code","is_day":""},"hourly":{"time":["2026-11-18T05:00","2026-11-18T06:00","2026-11-18T07:00","2026-11-18T08:00","2026-11-18T09:00","2026-11-18T10:00","2026-11-18T11:00","2026-11-18T12:00"],"temperature_2m":[19.7,19.4,19.2,19.1,19.4,19.5,19.5,19.8],"weather_code":[3,3,3,3,3,3,80,3],"is_day":[0,0,0,1,1,1,1,1]},"daily_units":{"time":"iso8601","temperature_2m_max":"°C","temperature_2m_min":"°C"},"daily":{"time":["2026-11-18","2026-11-19"],"temperature_2m_max":[19.9,17.9],"temperature_2m_min":[16.6,12.5]}}"#;

    fn hour(hour: u8, temperature: i32, condition: Condition, is_day: bool) -> Hour {
        Hour {
            hour,
            temperature,
            condition,
            is_day,
        }
    }

    #[test]
    fn parses_a_real_answer() {
        let forecast = parse(LISBON.as_bytes()).unwrap();
        assert_eq!(
            forecast,
            Forecast {
                temperature: 19,
                condition: Condition::Cloudy,
                is_day: false,
                high: 20,
                low: 17,
                hours: vec![
                    hour(7, 19, Condition::Cloudy, false),
                    hour(8, 19, Condition::Cloudy, true),
                    hour(9, 19, Condition::Cloudy, true),
                    hour(10, 20, Condition::Cloudy, true),
                ],
            }
        );
    }

    #[test]
    fn hours_run_past_midnight() {
        let body = r#"{"current":{"time":"2026-11-18T22:45","temperature_2m":-0.6,"weather_code":71,"is_day":0},
            "hourly":{"time":["2026-11-18T22:00","2026-11-18T23:00","2026-11-19T00:00","2026-11-19T01:00","2026-11-19T02:00","2026-11-19T03:00"],
            "temperature_2m":[0,-1,-2,-2.5,-3,-4],"weather_code":[71,73,0,1,2,95],"is_day":[0,0,0,0,0,0]},
            "daily":{"temperature_2m_max":[3.4],"temperature_2m_min":[-4.5]}}"#;
        let forecast = parse(body.as_bytes()).unwrap();
        assert_eq!(forecast.temperature, -1);
        assert_eq!(forecast.condition, Condition::Snow);
        assert_eq!((forecast.high, forecast.low), (3, -5));
        let hours: Vec<_> = forecast
            .hours
            .iter()
            .map(|h| (h.hour, h.temperature))
            .collect();
        assert_eq!(hours, [(23, -1), (0, -2), (1, -3), (2, -3)]);
    }

    #[test]
    fn a_short_forecast_keeps_what_there_is() {
        let body = r#"{"current":{"time":"2026-11-18T23:10","temperature_2m":5,"weather_code":0,"is_day":0},
            "hourly":{"time":["2026-11-18T23:00","2026-11-19T00:00"],"temperature_2m":[5,4],"weather_code":[0,0],"is_day":[0,0]},
            "daily":{"temperature_2m_max":[9],"temperature_2m_min":[2]}}"#;
        let forecast = parse(body.as_bytes()).unwrap();
        assert_eq!(forecast.hours, [hour(0, 4, Condition::Clear, false)]);
    }

    #[test]
    fn missing_fields_are_errors() {
        for field in [
            "\"current\":",
            "\"temperature_2m\":19.4",
            "\"is_day\":[",
            "\"hourly\":",
            "\"daily\":",
            "\"temperature_2m_max\":[",
        ] {
            assert_eq!(LISBON.matches(field).count(), 1, "{field}");
            let renamed = field.replacen(|c: char| c.is_ascii_lowercase(), "x", 1);
            let without = LISBON.replace(field, &renamed);
            assert!(parse(without.as_bytes()).is_err(), "{field}");
        }
        let no_days = LISBON.replace("[19.9,17.9]", "[]");
        assert!(parse(no_days.as_bytes()).is_err());
        assert!(parse(b"").is_err());
        assert!(parse(br#"{"error":true,"reason":"Latitude must be in range"}"#).is_err());
        let bad_time = LISBON.replace("2026-11-18T06:15", "06:15");
        assert!(parse(bad_time.as_bytes()).is_err());
    }

    #[test]
    fn codes_map_to_conditions() {
        let cases = [
            (0, Condition::Clear),
            (1, Condition::Clear),
            (2, Condition::PartlyCloudy),
            (3, Condition::Cloudy),
            (45, Condition::Fog),
            (48, Condition::Fog),
            (53, Condition::Drizzle),
            (57, Condition::Drizzle),
            (61, Condition::Rain),
            (67, Condition::Rain),
            (81, Condition::Rain),
            (71, Condition::Snow),
            (86, Condition::Snow),
            (95, Condition::Thunder),
            (99, Condition::Thunder),
            (42, Condition::Cloudy),
        ];
        for (code, condition) in cases {
            assert_eq!(Condition::from_code(code), condition, "code {code}");
        }
        assert_eq!(Condition::Clear.label(true), "Sunny");
        assert_eq!(Condition::Clear.label(false), "Clear");
    }

    #[test]
    fn path_rounds_the_position() {
        let celsius = path(38.746044, -9.175565, Unit::Celsius);
        assert!(celsius.contains("latitude=38.75&longitude=-9.18&"));
        assert!(!celsius.contains("temperature_unit"));
        assert!(!celsius.contains(char::is_whitespace));
        let fahrenheit = path(40.7128, -74.006, Unit::Fahrenheit);
        assert!(fahrenheit.ends_with("&timezone=auto&temperature_unit=fahrenheit"));
    }

    #[test]
    fn fetches_again_after_a_wait_that_depends_on_the_outcome() {
        let start = Instant::now();
        let mut weather = Weather::default();
        assert!(weather.due(start));
        assert_eq!(weather.show(), Show::Loading);

        weather.record(start, Err(Problem::NoLocation));
        assert_eq!(weather.show(), Show::LocationOff);
        assert!(!weather.due(start + Duration::from_secs(59)));
        assert!(weather.due(start + RETRY_NO_LOCATION));

        weather.record(start, Err(Problem::Failed));
        assert_eq!(weather.show(), Show::Unavailable);
        assert!(!weather.due(start + Duration::from_secs(299)));
        assert!(weather.due(start + RETRY));

        let forecast = parse(LISBON.as_bytes()).unwrap();
        weather.record(start, Ok(forecast.clone()));
        assert!(!weather.due(start + RETRY));
        assert!(weather.due(start + REFRESH));
        assert_eq!(weather.show(), Show::Forecast(&forecast));

        // A failure later keeps the last forecast on the card, and retries sooner.
        let later = start + REFRESH;
        weather.record(later, Err(Problem::Failed));
        assert_eq!(weather.show(), Show::Forecast(&forecast));
        assert!(weather.due(later + RETRY));
    }
}
