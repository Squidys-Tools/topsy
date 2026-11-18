//! The calendar widget's rules: civil dates in the proleptic Gregorian calendar, the month grid
//! for a given first day of the week, ISO-8601 week numbers, and the month being browsed.

/// A day of the week, Monday first as ISO-8601 numbers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

const WEEK: [Weekday; 7] = [
    Weekday::Monday,
    Weekday::Tuesday,
    Weekday::Wednesday,
    Weekday::Thursday,
    Weekday::Friday,
    Weekday::Saturday,
    Weekday::Sunday,
];

impl Weekday {
    /// 0 for Monday up to 6 for Sunday.
    pub fn index(self) -> u8 {
        match self {
            Weekday::Monday => 0,
            Weekday::Tuesday => 1,
            Weekday::Wednesday => 2,
            Weekday::Thursday => 3,
            Weekday::Friday => 4,
            Weekday::Saturday => 5,
            Weekday::Sunday => 6,
        }
    }

    /// The weekday `index` days after Monday, wrapping round the week.
    pub fn from_index(index: u32) -> Self {
        WEEK[(index % 7) as usize]
    }

    pub fn is_weekend(self) -> bool {
        match self {
            Weekday::Saturday | Weekday::Sunday => true,
            Weekday::Monday
            | Weekday::Tuesday
            | Weekday::Wednesday
            | Weekday::Thursday
            | Weekday::Friday => false,
        }
    }

    pub fn initial(self) -> char {
        match self {
            Weekday::Monday => 'M',
            Weekday::Tuesday | Weekday::Thursday => 'T',
            Weekday::Wednesday => 'W',
            Weekday::Friday => 'F',
            Weekday::Saturday | Weekday::Sunday => 'S',
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Weekday::Monday => "MON",
            Weekday::Tuesday => "TUE",
            Weekday::Wednesday => "WED",
            Weekday::Thursday => "THU",
            Weekday::Friday => "FRI",
            Weekday::Saturday => "SAT",
            Weekday::Sunday => "SUN",
        }
    }
}

/// The seven days of a week starting on `first`.
pub fn week_from(first: Weekday) -> [Weekday; 7] {
    std::array::from_fn(|i| Weekday::from_index(u32::from(first.index()) + i as u32))
}

/// A month of a year; `month` is 1 to 12.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YearMonth {
    pub year: i32,
    pub month: u8,
}

impl YearMonth {
    /// `months` later, or earlier when negative, across year ends.
    pub fn step(self, months: i32) -> Self {
        let index = self.year * 12 + i32::from(self.month) - 1 + months;
        Self {
            year: index.div_euclid(12),
            month: (index.rem_euclid(12) + 1) as u8,
        }
    }

    pub fn days(self) -> u8 {
        match self.month {
            2 if is_leap(self.year) => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        }
    }

    pub fn short(self) -> &'static str {
        const NAMES: [&str; 12] = [
            "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
        ];
        NAMES[usize::from(self.month.clamp(1, 12) - 1)]
    }

    /// The month as weeks starting on `first`, each day its date, blank outside the month.
    pub fn grid(self, first: Weekday) -> Vec<[Option<u8>; 7]> {
        let start = Date::new(self.year, self.month, 1).weekday();
        let lead = (7 + start.index() - first.index()) % 7;
        let cells = usize::from(lead) + usize::from(self.days());
        (0..cells.div_ceil(7))
            .map(|week| {
                std::array::from_fn(|column| {
                    let day = (week * 7 + column) as i32 - i32::from(lead) + 1;
                    (1..=i32::from(self.days()))
                        .contains(&day)
                        .then_some(day as u8)
                })
            })
            .collect()
    }
}

pub fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl Date {
    pub fn new(year: i32, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }

    pub fn year_month(self) -> YearMonth {
        YearMonth {
            year: self.year,
            month: self.month,
        }
    }

    /// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
    fn days_since_epoch(self) -> i64 {
        let month = i64::from(self.month);
        let year = i64::from(self.year) - i64::from(month <= 2);
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let day_of_year =
            (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    pub fn weekday(self) -> Weekday {
        // 1970-01-01 was a Thursday.
        Weekday::from_index((self.days_since_epoch() + 3).rem_euclid(7) as u32)
    }

    /// The ISO-8601 week: its year, which near New Year can differ from the date's, and number.
    pub fn iso_week(self) -> (i32, u8) {
        let ordinal = self.days_since_epoch() - Date::new(self.year, 1, 1).days_since_epoch() + 1;
        let week = (ordinal - i64::from(self.weekday().index()) + 9) / 7;
        if week < 1 {
            (self.year - 1, weeks_in(self.year - 1))
        } else if week > i64::from(weeks_in(self.year)) {
            (self.year + 1, 1)
        } else {
            (self.year, week as u8)
        }
    }
}

/// ISO weeks in a year: 53 when it starts on a Thursday, or on a Wednesday in a leap year.
fn weeks_in(year: i32) -> u8 {
    match Date::new(year, 1, 1).weekday() {
        Weekday::Thursday => 53,
        Weekday::Wednesday if is_leap(year) => 53,
        Weekday::Monday
        | Weekday::Tuesday
        | Weekday::Wednesday
        | Weekday::Friday
        | Weekday::Saturday
        | Weekday::Sunday => 52,
    }
}

/// The month being looked at: some months away from the month it was browsed from. Once that
/// month is over the browse is stale and the calendar shows the current month again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Browse {
    from: Option<YearMonth>,
    offset: i32,
}

impl Browse {
    pub fn shown(self, this_month: YearMonth) -> YearMonth {
        match self.from {
            Some(from) if from == this_month => this_month.step(self.offset),
            Some(_) | None => this_month,
        }
    }

    pub fn step(&mut self, months: i32, this_month: YearMonth) {
        if self.from != Some(this_month) {
            *self = Self::default();
        }
        self.from = Some(this_month);
        self.offset += months;
    }

    pub fn is_away(self, this_month: YearMonth) -> bool {
        self.shown(this_month) != this_month
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ym(year: i32, month: u8) -> YearMonth {
        YearMonth { year, month }
    }

    #[test]
    fn days_in_months_follow_the_leap_rules() {
        assert_eq!(ym(2024, 2).days(), 29);
        assert_eq!(ym(2023, 2).days(), 28);
        assert_eq!(ym(2100, 2).days(), 28);
        assert_eq!(ym(2000, 2).days(), 29);
        assert_eq!(ym(1900, 2).days(), 28);
        assert_eq!(ym(2026, 1).days(), 31);
        assert_eq!(ym(2026, 4).days(), 30);
        assert_eq!(ym(2026, 11).days(), 30);
        assert_eq!(ym(2026, 12).days(), 31);
    }

    #[test]
    fn weekdays_of_known_dates() {
        assert_eq!(Date::new(1970, 1, 1).weekday(), Weekday::Thursday);
        assert_eq!(Date::new(2000, 1, 1).weekday(), Weekday::Saturday);
        assert_eq!(Date::new(2000, 2, 29).weekday(), Weekday::Tuesday);
        assert_eq!(Date::new(2024, 2, 29).weekday(), Weekday::Thursday);
        assert_eq!(Date::new(2026, 11, 18).weekday(), Weekday::Wednesday);
        assert_eq!(Date::new(2026, 10, 25).weekday(), Weekday::Sunday);
        assert_eq!(Date::new(1969, 12, 31).weekday(), Weekday::Wednesday);
    }

    #[test]
    fn grid_starts_on_the_first_weekday() {
        // November 2026 starts on a Sunday.
        let monday = ym(2026, 11).grid(Weekday::Monday);
        assert_eq!(monday.len(), 6);
        assert_eq!(monday[0], [None, None, None, None, None, None, Some(1)]);
        assert_eq!(monday[5], [Some(30), None, None, None, None, None, None]);
        let sunday = ym(2026, 11).grid(Weekday::Sunday);
        assert_eq!(sunday.len(), 5);
        assert_eq!(sunday[0][0], Some(1));
        assert_eq!(sunday[4][1], Some(30));
        assert_eq!(sunday[4][2], None);
        let saturday = ym(2026, 11).grid(Weekday::Saturday);
        assert_eq!(saturday[0][..2], [None, Some(1)]);
    }

    #[test]
    fn grids_hold_every_day_once_in_four_to_six_weeks() {
        for year in [1999, 2000, 2015, 2024, 2100] {
            for month in 1..=12 {
                for first in WEEK {
                    let grid = ym(year, month).grid(first);
                    assert!((4..=6).contains(&grid.len()));
                    let days: Vec<u8> = grid.iter().flatten().flatten().copied().collect();
                    assert_eq!(days, (1..=ym(year, month).days()).collect::<Vec<_>>());
                    assert!(grid[0].iter().any(Option::is_some));
                    assert!(grid[grid.len() - 1].iter().any(Option::is_some));
                }
            }
        }
        // February 2015 starts on a Sunday: exactly four rows.
        assert_eq!(ym(2015, 2).grid(Weekday::Sunday).len(), 4);
        // August 2026 starts on a Saturday and has 31 days: six rows from Monday.
        assert_eq!(ym(2026, 8).grid(Weekday::Monday).len(), 6);
    }

    #[test]
    fn week_initials_follow_the_first_day() {
        let initials = |first| week_from(first).map(Weekday::initial);
        assert_eq!(
            initials(Weekday::Monday),
            ['M', 'T', 'W', 'T', 'F', 'S', 'S']
        );
        assert_eq!(
            initials(Weekday::Sunday),
            ['S', 'M', 'T', 'W', 'T', 'F', 'S']
        );
        assert_eq!(week_from(Weekday::Saturday)[1], Weekday::Sunday);
    }

    #[test]
    fn iso_weeks_of_known_dates() {
        assert_eq!(Date::new(2026, 11, 18).iso_week(), (2026, 47));
        assert_eq!(Date::new(2026, 10, 25).iso_week(), (2026, 43));
        assert_eq!(Date::new(2024, 1, 1).iso_week(), (2024, 1));
        assert_eq!(Date::new(2024, 12, 30).iso_week(), (2025, 1));
        // 2020 starts on a Wednesday in a leap year, 2015 and 2026 on a Thursday: 53 weeks.
        assert_eq!(Date::new(2020, 12, 31).iso_week(), (2020, 53));
        assert_eq!(Date::new(2015, 12, 31).iso_week(), (2015, 53));
        assert_eq!(Date::new(2026, 12, 31).iso_week(), (2026, 53));
        assert_eq!(Date::new(2027, 1, 3).iso_week(), (2026, 53));
        assert_eq!(Date::new(2027, 1, 4).iso_week(), (2027, 1));
        // Jan 1 in the last week of the year before.
        assert_eq!(Date::new(2021, 1, 1).iso_week(), (2020, 53));
        assert_eq!(Date::new(2023, 1, 1).iso_week(), (2022, 52));
        assert_eq!(Date::new(2022, 1, 2).iso_week(), (2021, 52));
        assert_eq!(Date::new(2019, 12, 30).iso_week(), (2020, 1));
    }

    #[test]
    fn stepping_months_crosses_year_ends() {
        assert_eq!(ym(2026, 12).step(1), ym(2027, 1));
        assert_eq!(ym(2026, 1).step(-1), ym(2025, 12));
        assert_eq!(ym(2026, 11).step(14), ym(2028, 1));
        assert_eq!(ym(2026, 11).step(-23), ym(2024, 12));
        assert_eq!(ym(0, 1).step(-1), ym(-1, 12));
        assert_eq!(ym(2026, 5).step(0), ym(2026, 5));
    }

    #[test]
    fn a_browse_goes_stale_when_the_month_turns() {
        let november = ym(2026, 11);
        let mut browse = Browse::default();
        assert_eq!(browse.shown(november), november);
        browse.step(-1, november);
        browse.step(-1, november);
        assert_eq!(browse.shown(november), ym(2026, 9));
        assert!(browse.is_away(november));
        let december = ym(2026, 12);
        assert_eq!(browse.shown(december), december);
        browse.step(1, december);
        assert_eq!(browse.shown(december), ym(2027, 1));
        browse.step(-1, december);
        assert!(!browse.is_away(december));
    }
}
