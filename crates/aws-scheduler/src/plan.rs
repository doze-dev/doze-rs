//! When a schedule next fires.
//!
//! - `at()` fires once, at its wall clock reading in the schedule's zone; `StartDate` and `EndDate` do not apply (documented).
//! - `rate()` counts real time from `StartDate`, or from when the schedule was made or last updated; with no `StartDate` the first
//!   fire is at once (documented).
//! - `cron()` is read on the zone's clock (a skipped time does not fire, a repeated one fires at its second showing: `aws-cron`).
//! - A recurring schedule fires at or after `StartDate` and before `EndDate` (documented: "the date, in UTC, before which the schedule
//!   can invoke its target").
//!
//! **Flexible time windows** fire at the start of the window, the scheduled time itself. AWS picks a point anywhere in the window;
//! every point is a valid outcome, and the start is the one a developer can predict and a test can wait for. A developer whose code
//! must tolerate the spread reads `MaximumWindowInMinutes` back; doze does not invent a random delay.

use aws_cron::{Schedule as Expr, Zone};

use crate::model::Schedule;

/// The first fire of `s` after `after_ms`; `None` when it is disabled or never fires again.
pub(crate) fn next(s: &Schedule, after_ms: i64) -> Option<i64> {
    if !s.enabled {
        return None;
    }
    let expr = aws_cron::parse_scheduler(&s.expression).ok()?;
    let zone = s
        .timezone
        .as_deref()
        .and_then(Zone::named)
        .unwrap_or_else(Zone::utc);
    if expr.is_once() {
        return expr.next_in(&zone, 0, after_ms);
    }
    let from = match s.start_ms {
        Some(start) => after_ms.max(start - 1),
        None => after_ms,
    };
    let at = match expr {
        Expr::Rate { .. } => {
            let anchor = s.start_ms.unwrap_or(s.anchor_ms);
            if from < anchor {
                Some(anchor)
            } else {
                expr.next_in(&zone, anchor, from)
            }
        }
        _ => expr.next_in(&zone, 0, from),
    }?;
    s.end_ms.is_none_or(|end| at < end).then_some(at)
}
