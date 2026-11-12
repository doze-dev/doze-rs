//! The two day fields. Exactly one of them is `?`; the other says which days of the month count.

use crate::Invalid;
use crate::civil::{days_in_month, weekday};
use crate::field::{Bits, DAY_OF_MONTH, DAY_OF_WEEK, plain};

/// Day-of-month: numbers, and the specials `L` (last day), `LW` (last weekday) and `nW` (the weekday nearest day n).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DayOfMonth {
    set: Bits,
    last: bool,
    last_weekday: bool,
    near_weekday: Vec<u32>,
}

/// Day-of-week: numbers (1 is Sunday), `L` (Saturday), `nL` (the last weekday n of the month) and `n#m` (the m-th weekday n).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DayOfWeek {
    set: Bits,
    last_of: Vec<u32>,
    nth: Vec<(u32, u32)>,
}

/// Which days count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Days {
    OfMonth(DayOfMonth),
    OfWeek(DayOfWeek),
}

/// Reads the two fields.
pub(crate) fn parse(dom: &str, dow: &str) -> Result<Days, Invalid> {
    match (dom == "?", dow == "?") {
        (true, false) => day_of_week(dow).map(Days::OfWeek),
        (false, true) => day_of_month(dom).map(Days::OfMonth),
        _ => Err(Invalid),
    }
}

fn day_of_month(text: &str) -> Result<DayOfMonth, Invalid> {
    let mut d = DayOfMonth {
        set: Bits::default(),
        last: false,
        last_weekday: false,
        near_weekday: Vec::new(),
    };
    for element in text.split(',') {
        if element == "L" {
            d.last = true;
        } else if element == "LW" {
            d.last_weekday = true;
        } else if let Some(n) = element.strip_suffix('W') {
            d.near_weekday.push(DAY_OF_MONTH.value(n)?);
        } else {
            plain(&DAY_OF_MONTH, element, &mut d.set)?;
        }
    }
    Ok(d)
}

fn day_of_week(text: &str) -> Result<DayOfWeek, Invalid> {
    let mut d = DayOfWeek {
        set: Bits::default(),
        last_of: Vec::new(),
        nth: Vec::new(),
    };
    for element in text.split(',') {
        if element == "L" {
            d.set.set(7);
        } else if let Some(n) = element.strip_suffix('L') {
            d.last_of.push(DAY_OF_WEEK.value(n)?);
        } else if let Some((n, m)) = element.split_once('#') {
            let m: u32 = m.parse().map_err(|_| Invalid)?;
            if !(1..=5).contains(&m) {
                return Err(Invalid);
            }
            d.nth.push((DAY_OF_WEEK.value(n)?, m));
        } else {
            plain(&DAY_OF_WEEK, element, &mut d.set)?;
        }
    }
    Ok(d)
}

impl Days {
    /// Whether the day counts. `days` is the day as days since the epoch.
    pub(crate) fn matches(&self, year: i64, month: u32, day: u32, days: i64) -> bool {
        let last = days_in_month(year, month);
        // The weekday of day `d` of this month.
        let wd = |d: u32| weekday(days + i64::from(d) - i64::from(day));
        match self {
            Self::OfMonth(d) => d.matches(day, last, wd),
            Self::OfWeek(d) => d.matches(day, last, wd(day)),
        }
    }
}

/// Monday to Friday (Sunday is 1).
const fn is_weekday(w: u32) -> bool {
    matches!(w, 2..=6)
}

impl DayOfMonth {
    fn matches(&self, day: u32, last: u32, wd: impl Fn(u32) -> u32) -> bool {
        if self.set.has(day) || (self.last && day == last) {
            return true;
        }
        if self.last_weekday && (1..=last).rev().find(|d| is_weekday(wd(*d))) == Some(day) {
            return true;
        }
        self.near_weekday
            .iter()
            .any(|n| *n <= last && nearest_weekday(*n, last, &wd) == day)
    }
}

/// The weekday nearest day `target`, without leaving the month (Quartz's `W`).
fn nearest_weekday(target: u32, last: u32, wd: &impl Fn(u32) -> u32) -> u32 {
    match wd(target) {
        7 if target == 1 => 3,
        7 => target - 1,
        1 if target == last => target.saturating_sub(2),
        1 => target + 1,
        _ => target,
    }
}

impl DayOfWeek {
    fn matches(&self, day: u32, last: u32, dow: u32) -> bool {
        self.set.has(dow)
            || (day + 7 > last && self.last_of.contains(&dow))
            || self
                .nth
                .iter()
                .any(|(n, m)| *n == dow && (day - 1) / 7 + 1 == *m)
    }
}
