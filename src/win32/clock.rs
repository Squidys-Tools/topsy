//! The local date and the user's first day of the week, for the calendar widget. Both are cheap
//! enough to read on every render.

use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_IFIRSTDAYOFWEEK};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::core::PCWSTR;

use model::calendar::{Date, Weekday};

pub fn today() -> Date {
    let now = unsafe { GetLocalTime() };
    Date::new(i32::from(now.wYear), now.wMonth as u8, now.wDay as u8)
}

/// The first day of the week in the user's regional settings, or Monday if they can't be read.
pub fn first_weekday() -> Weekday {
    let mut value = [0u16; 4];
    // A null name is LOCALE_NAME_USER_DEFAULT; the value is "0" for Monday up to "6" for Sunday.
    let written =
        unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_IFIRSTDAYOFWEEK, Some(&mut value)) };
    match (
        written,
        char::from_u32(u32::from(value[0])).and_then(|c| c.to_digit(10)),
    ) {
        (1.., Some(digit @ 0..=6)) => Weekday::from_index(digit),
        _ => Weekday::Monday,
    }
}
