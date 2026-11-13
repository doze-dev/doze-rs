//! EventBridge schedule expressions. `rate(5 minutes)` fires every period from the moment the rule was made; `cron(15 10 * * ? *)` is
//! the six-field expression `minutes hours day-of-month month day-of-week year`, in UTC, with `,` `-` `*` `/`, month and weekday names,
//! `?` in one of the two day fields, and `L`, `W` and `#`.
//!
//! The evaluator is pure: [`Schedule::next_after`] says when an expression next fires after a given instant, so a caller sleeps to that
//! deadline and holds no timer when there is none. Nothing here reads a clock.

mod civil;
mod days;
mod field;

use civil::Civil;
use days::Days;
use field::{Bits, HOUR, MINUTE, MONTH, YEAR};

/// An expression AWS would refuse. The message AWS gives is the same whatever is wrong, so the type says nothing more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Invalid;

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the schedule expression is not valid")
    }
}

impl std::error::Error for Invalid {}

/// A parsed `rate(...)` or `cron(...)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Schedule {
    /// Every `every_ms`, counted from the anchor (when the rule was made).
    Rate { every_ms: i64 },
    /// A cron expression.
    Cron(Cron),
}

/// A `cron(...)` expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cron {
    minutes: Bits,
    hours: Bits,
    days: Days,
    months: Bits,
    years: Bits,
}

// fuzz:parser fuzz_schedule
/// Reads an expression.
///
/// # Errors
/// [`Invalid`] for anything AWS refuses: the wrong unit or plural for a rate, a value out of range, both or neither of the day fields
/// `?`, a malformed list.
pub fn parse(text: &str) -> Result<Schedule, Invalid> {
    let inner = |prefix: &str| text.strip_prefix(prefix)?.strip_suffix(')');
    if let Some(body) = inner("rate(") {
        return parse_rate(body);
    }
    if let Some(body) = inner("cron(") {
        return parse_cron(body).map(Schedule::Cron);
    }
    Err(Invalid)
}

/// `5 minutes`, `1 hour`: a positive whole number and a unit that is singular exactly when the number is 1.
fn parse_rate(body: &str) -> Result<Schedule, Invalid> {
    let (n, unit) = body.split_once(' ').ok_or(Invalid)?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) || n.len() > 9 {
        return Err(Invalid);
    }
    let n: i64 = n.parse().map_err(|_| Invalid)?;
    if n == 0 {
        return Err(Invalid);
    }
    let (unit_ms, name) = match unit.trim_end_matches('s') {
        "minute" => (60_000, "minute"),
        "hour" => (3_600_000, "hour"),
        "day" => (86_400_000, "day"),
        _ => return Err(Invalid),
    };
    let plural = n != 1;
    if unit
        != if plural {
            format!("{name}s")
        } else {
            name.to_owned()
        }
    {
        return Err(Invalid);
    }
    Ok(Schedule::Rate {
        every_ms: n * unit_ms,
    })
}

fn parse_cron(body: &str) -> Result<Cron, Invalid> {
    let fields: Vec<&str> = body.split(' ').collect();
    let [minutes, hours, dom, month, dow, year] = fields.as_slice() else {
        return Err(Invalid);
    };
    Ok(Cron {
        minutes: field::plain_field(&MINUTE, minutes)?,
        hours: field::plain_field(&HOUR, hours)?,
        days: days::parse(dom, dow)?,
        months: field::plain_field(&MONTH, month)?,
        years: field::plain_field(&YEAR, year)?,
    })
}

impl Schedule {
    /// The first instant after `after_ms` (milliseconds since the epoch) at which the schedule fires. A rate counts from `anchor_ms`;
    /// a cron expression ignores it. `None` when a cron expression never fires again (its last year has passed).
    #[must_use]
    pub fn next_after(&self, anchor_ms: i64, after_ms: i64) -> Option<i64> {
        match self {
            Self::Rate { every_ms } => {
                let elapsed = after_ms.saturating_sub(anchor_ms);
                let periods = if elapsed < 0 { 0 } else { elapsed / every_ms };
                anchor_ms.checked_add(periods.checked_add(1)?.checked_mul(*every_ms)?)
            }
            Self::Cron(c) => c.next_after(after_ms),
        }
    }
}

/// The last year a cron expression can name.
const LAST_YEAR: i64 = 2199;

impl Cron {
    fn next_after(&self, after_ms: i64) -> Option<i64> {
        let mut at = Civil::of_ms(after_ms.checked_add(60_000)?);
        while at.year <= LAST_YEAR {
            at = match self.step(at) {
                Step::Found => return Some(at.to_ms()),
                Step::Moved(next) => next,
            };
        }
        None
    }

    /// Checks `at` against each field in turn, largest first. A field that does not match moves `at` to the start of the next unit
    /// that does.
    fn step(&self, at: Civil) -> Step {
        // Years are kept as the distance from 1970.
        let year = u32::try_from(at.year - 1970).unwrap_or(0);
        if !self.years.has(year) {
            let next = self
                .years
                .from(year + 1, 2199 - 1970)
                .map_or(LAST_YEAR + 1, |y| i64::from(y) + 1970);
            return Step::Moved(start_of(next, 1, 1));
        }
        if !self.months.has(at.month) {
            return Step::Moved(next_month(at));
        }
        if !self.days.matches(at.year, at.month, at.day, at.days()) {
            return Step::Moved(next_day(at));
        }
        if !self.hours.has(at.hour) {
            return Step::Moved(match self.hours.from(at.hour + 1, 23) {
                Some(h) => Civil {
                    hour: h,
                    minute: 0,
                    ..at
                },
                None => next_day(at),
            });
        }
        match self.minutes.from(at.minute, 59) {
            Some(m) if m == at.minute => Step::Found,
            Some(m) => Step::Moved(Civil { minute: m, ..at }),
            None => Step::Moved(Civil {
                minute: 0,
                ..next_hour(at)
            }),
        }
    }
}

enum Step {
    Found,
    Moved(Civil),
}

fn start_of(year: i64, month: u32, day: u32) -> Civil {
    Civil {
        year,
        month,
        day,
        hour: 0,
        minute: 0,
    }
}

fn next_month(at: Civil) -> Civil {
    if at.month == 12 {
        start_of(at.year + 1, 1, 1)
    } else {
        start_of(at.year, at.month + 1, 1)
    }
}

fn next_day(at: Civil) -> Civil {
    if at.day >= civil::days_in_month(at.year, at.month) {
        next_month(at)
    } else {
        start_of(at.year, at.month, at.day + 1)
    }
}

fn next_hour(at: Civil) -> Civil {
    if at.hour >= 23 {
        next_day(at)
    } else {
        Civil {
            hour: at.hour + 1,
            minute: 0,
            ..at
        }
    }
}

#[cfg(test)]
mod tests;
