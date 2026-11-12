//! The sentences of the Live pages. The wording is the boards'; the numbers are the recorder's, and the window is computed
//! from the oldest row, never promised.

use crate::format::{civil, clock, number, span};
use crate::model::{LiveState, LiveView, Span, WireWindow};
use crate::recorder::ring::ROWS;

/// `19:12`, UTC.
#[must_use]
pub fn hhmm(ms: i64) -> String {
    let c = civil(ms);
    format!("{:02}:{:02}", c.hour, c.minute)
}

/// `2,000`.
fn capacity() -> String {
    number(i64::try_from(ROWS).unwrap_or(0))
}

/// How far back the tape honestly reaches: `19:12, 3 h 2 min`; `None` when it is empty.
#[must_use]
pub fn reach(w: &WireWindow, now_ms: i64) -> Option<String> {
    let oldest = w.oldest_ms?;
    Some(format!(
        "{}, {}",
        hhmm(oldest),
        span((now_ms - oldest) / 1000)
    ))
}

/// The requests in the last minute.
#[must_use]
pub fn last_minute(n: u32) -> String {
    format!(
        "{} {} in the last minute.",
        number(i64::from(n)),
        if n == 1 { "request" } else { "requests" }
    )
}

/// The sentence under the title of Live (the board's `live-sentence`), in each state.
#[must_use]
pub fn live_sentence(v: &LiveView) -> Vec<Span> {
    match v.state {
        LiveState::Quiet => vec![Span::text(format!(
            "Nothing has reached the broker{} It is listening on {}. The wire starts empty at every start and keeps up to {} rows.",
            v.started_ms.map_or_else(
                || " since it started.".to_owned(),
                |ms| format!(" since it started at {}.", hhmm(ms))
            ),
            v.listen,
            capacity()
        ))],
        _ if v.paused_at_ms.is_some() => vec![Span::text(format!(
            "Paused at {}. Everything below is as it was then.",
            paused_clock(v.paused_at_ms.unwrap_or(0))
        ))],
        LiveState::Busy => {
            let top = v.busiest.as_ref().map_or_else(String::new, |(t, n)| {
                format!(", {} of them to {t}", number(i64::from(*n)))
            });
            vec![Span::text(format!(
                "A busy minute: {} requests{top}. Repeats fold into one row with a count and the span they cover. {}",
                number(i64::from(v.last_minute)),
                reach_sentence(v)
            ))]
        }
        LiveState::Steady => vec![Span::text(format!(
            "Everything your apps send, newest first. The broker keeps up to its last {} rows in memory and older ones fall off; repeats fold into one row, so quiet apps cost little. {} {}",
            capacity(),
            reach_sentence(v),
            last_minute(v.last_minute)
        ))],
    }
}

fn reach_sentence(v: &LiveView) -> String {
    reach(&v.window, v.now_ms).map_or_else(String::new, |r| {
        format!("Right now they reach back to {r}.")
    })
}

/// `22:14:23.6`: the paused-at time, to a tenth of a second.
#[must_use]
pub fn paused_clock(ms: i64) -> String {
    format!("{}.{}", clock(ms), ms.rem_euclid(1000) / 100)
}

/// The line under the table: `Showing the newest 16 of 2,000 rows · back to 19:12 · idle polls hidden`.
#[must_use]
pub fn pager_line(v: &LiveView) -> String {
    let mut out = format!(
        "Showing {} of {} matching rows",
        number(i64::try_from(v.rows.len()).unwrap_or(0)),
        number(i64::try_from(v.matched).unwrap_or(0))
    );
    if let Some(oldest) = v.window.oldest_ms {
        out.push_str(&format!(" · back to {}", hhmm(oldest)));
    }
    if v.query.hide_idle {
        out.push_str(" · idle polls hidden");
    }
    out
}

/// The load line of the busy state: `7 rows stand for 571 requests.`
#[must_use]
pub fn load_line(rows: usize, requests: u64) -> String {
    format!(
        "Repeats of the same call fold into one row. {} rows stand for {} requests.",
        number(i64::try_from(rows).unwrap_or(0)),
        number(i64::try_from(requests).unwrap_or(0))
    )
}

#[cfg(test)]
mod tests;
