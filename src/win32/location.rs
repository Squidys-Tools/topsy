//! Where the machine is, for the weather: Windows' location service, the user's measurement
//! system, and the Location privacy page. The position calls block: worker thread only.

use std::path::Path;

use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator, PositionAccuracy};
use windows::Foundation::TimeSpan;
use windows::Win32::Foundation::E_ACCESSDENIED;
use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_IMEASURE};
use windows::Win32::System::Com::COINIT_MULTITHREADED;
use windows::core::PCWSTR;

use super::{open_path, with_com};

const SETTINGS: &str = "ms-settings:privacy-location";
/// `TimeSpan` counts 100 ns ticks.
const TICKS_PER_SECOND: i64 = 10_000_000;
/// A position this recent is good enough for a forecast.
const MAX_AGE_SECONDS: i64 = 30 * 60;
const TIMEOUT_SECONDS: i64 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Location is off for the machine or for desktop apps, or the user refused it.
    Off,
    /// Allowed, but no position came (no Wi-Fi to place it by, timed out).
    Unavailable,
}

/// The machine's position as (latitude, longitude), coarse. Blocks for up to about ten seconds.
pub fn locate() -> Result<(f64, f64), Error> {
    with_com(COINIT_MULTITHREADED, || {
        let access = Geolocator::RequestAccessAsync()
            .and_then(|operation| operation.join())
            .map_err(|err| {
                eprintln!("topsy: location access: {err}");
                Error::Unavailable
            })?;
        if access != GeolocationAccessStatus::Allowed {
            return Err(Error::Off);
        }
        position().map_err(|err| {
            eprintln!("topsy: no position: {err}");
            if err.code() == E_ACCESSDENIED {
                Error::Off
            } else {
                Error::Unavailable
            }
        })
    })
}

fn position() -> windows::core::Result<(f64, f64)> {
    let locator = Geolocator::new()?;
    locator.SetDesiredAccuracy(PositionAccuracy::Default)?;
    let span = |seconds: i64| TimeSpan {
        Duration: seconds * TICKS_PER_SECOND,
    };
    let place = locator
        .GetGeopositionAsyncWithAgeAndTimeout(span(MAX_AGE_SECONDS), span(TIMEOUT_SECONDS))?
        .join()?
        .Coordinate()?
        .Point()?
        .Position()?;
    Ok((place.Latitude, place.Longitude))
}

/// Whether the user's locale measures in US units, so temperatures read in Fahrenheit.
pub fn us_units() -> bool {
    let mut value = [0u16; 2];
    // A null locale name is the user's default locale.
    let written = unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_IMEASURE, Some(&mut value)) };
    written > 0 && value[0] == u16::from(b'1')
}

/// Open Settings at Location privacy. Pumps messages like `open_path`: call it from a thread of
/// its own.
pub fn open_settings() -> bool {
    open_path(Path::new(SETTINGS))
}
