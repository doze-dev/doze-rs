//! EventBridge Scheduler's expressions: `at()` besides `rate()` and `cron()`, evaluated in a time zone.
//!
//! The rules are AWS's documented ones (EventBridge Scheduler User Guide, "Schedule types", read 2026-11-14):
//! - a cron expression is read on the zone's wall clock. A time the clock skips (spring forward) is **skipped**; a time it shows twice
//!   (fall back) fires **once, at the second showing**;
//! - `rate()` counts real time (`rate(1 days)` is 24 hours whatever the clock does);
//! - `at()` is a wall clock reading in the zone. A reading the clock skips is moved on by the gap, and one it shows twice is the
//!   second showing, as for cron (neither is documented: memory/choice).

use crate::civil::days_from_civil;
use crate::tz::{Local, Zone};
use crate::{Cron, Invalid, Schedule};

/// The most wall clock candidates looked at for one answer: enough for a schedule that fires every minute to cross a two-hour fold.
const MAX_CANDIDATES: usize = 4096;

// fuzz:parser fuzz_schedule
/// Reads an EventBridge Scheduler expression: `rate(...)`, `cron(...)` or `at(yyyy-mm-ddThh:mm:ss)`.
///
/// # Errors
/// [`Invalid`] for anything AWS refuses.
pub fn parse_scheduler(text: &str) -> Result<Schedule, Invalid> {
    if let Some(body) = text.strip_prefix("at(").and_then(|t| t.strip_suffix(')')) {
        return parse_at(body).map(|wall_ms| Schedule::At { wall_ms });
    }
    crate::parse(text)
}

/// `2026-11-20T13:00:00` as milliseconds on a clock that reads it as UTC.
fn parse_at(body: &str) -> Result<i64, Invalid> {
    let b = body.as_bytes();
    let shape_ok = b.len() == 19
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b'T',
            13 | 16 => *c == b':',
            _ => c.is_ascii_digit(),
        });
    if !shape_ok {
        return Err(Invalid);
    }
    let n = |from: usize, to: usize| -> i64 { body[from..to].parse().unwrap_or(-1) };
    let (year, month, day) = (n(0, 4), n(5, 7), n(8, 10));
    let (hour, minute, second) = (n(11, 13), n(14, 16), n(17, 19));
    let month_u = u32::try_from(month).map_err(|_| Invalid)?;
    let day_u = u32::try_from(day).map_err(|_| Invalid)?;
    let ok = (1..=12).contains(&month)
        && day >= 1
        && day_u <= crate::civil::days_in_month(year, month_u)
        && (0..24).contains(&hour)
        && (0..60).contains(&minute)
        && (0..60).contains(&second);
    if !ok {
        return Err(Invalid);
    }
    let days = days_from_civil(year, month_u, day_u);
    Ok(((days * 24 + hour) * 60 + minute) * 60_000 + second * 1000)
}

impl Schedule {
    /// The first instant after `after_ms` at which the schedule fires, read in `zone`. A rate counts real time from `anchor_ms`.
    /// `None` when it never fires again.
    #[must_use]
    pub fn next_in(&self, zone: &Zone, anchor_ms: i64, after_ms: i64) -> Option<i64> {
        match self {
            Self::Rate { .. } => self.next_after(anchor_ms, after_ms),
            Self::At { wall_ms } => {
                let at = match zone.resolve(*wall_ms) {
                    Local::Unique(t) | Local::Gap(t) | Local::Fold(_, t) => t,
                };
                (at > after_ms).then_some(at)
            }
            Self::Cron(c) => c.next_in(zone, after_ms),
        }
    }

    /// Whether the schedule fires once (`at()`).
    #[must_use]
    pub const fn is_once(&self) -> bool {
        matches!(self, Self::At { .. })
    }
}

impl Cron {
    fn next_in(&self, zone: &Zone, after_ms: i64) -> Option<i64> {
        // No instant after `after_ms` reads earlier than this on the zone's clock; start a minute before it.
        let mut from = after_ms
            .checked_add(zone.min_offset_ms())?
            .checked_sub(60_000)?;
        for _ in 0..MAX_CANDIDATES {
            let wall = self.next_after(from)?;
            match zone.resolve(wall) {
                Local::Gap(_) => {}
                Local::Unique(t) | Local::Fold(_, t) if t > after_ms => return Some(t),
                Local::Unique(_) | Local::Fold(..) => {}
            }
            from = wall;
        }
        None
    }
}

#[cfg(test)]
mod tests;
