//! Retention as sentences: when the oldest segment goes, what the next pass does, and what the last one did.

use super::planner::{Rule, RuleTone};
use super::{every, flag, number_of};
use crate::format::{bytes, clock, date_time, number, span_ms};
use crate::model::{LogRetentionView, LogSegment, LogView, Span};

fn t(s: impl Into<String>) -> Span {
    Span::text(s)
}

fn m(s: impl Into<String>) -> Span {
    Span::mono(s)
}

/// The oldest segment that is past retention now or will be, and when its turn comes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "LogExpiry"))]
pub struct Expiry {
    /// The segments (counting from the oldest, as the page numbers them) a pass run now would delete.
    pub going_now: usize,
    /// The first segment that survives it: the one whose first offset is the log start after.
    pub new_start: i64,
    /// The first segment that is not yet past retention, and when it turns past it (its newest record plus `retention.ms`).
    pub next: Option<(usize, i64)>,
    /// The time of the first pass at or after `next`'s time, when the passes are known (one has run).
    pub pass_at_ms: Option<i64>,
}

fn newest(s: &LogSegment) -> i64 {
    s.newest_ms.unwrap_or(0)
}

/// When the first pass at or after `turns_ms` is, given the last pass and the interval.
fn pass_after(r: &LogRetentionView, turns_ms: i64, now_ms: i64) -> Option<i64> {
    let last = r.last_pass.as_ref()?.at_ms;
    let step = r.every_ms.max(1);
    let from = turns_ms.max(now_ms);
    let k = (from - last + step - 1).div_euclid(step).max(1);
    Some(last + k * step)
}

/// What the broker's own rule says of the log's age and size, read for the page.
#[must_use]
pub fn expiry(v: &LogView) -> Option<Expiry> {
    let r = v.retention.as_ref()?;
    let going = r.would_delete;
    let next = r.max_age_ms.and_then(|age| {
        v.segments
            .iter()
            .enumerate()
            .skip(going)
            .find(|(_, s)| s.bytes > 0)
            .map(|(i, s)| (i, newest(s) + age))
    });
    // What the log start becomes: past the segments going now, else past the one that goes next.
    let new_start = if going > 0 {
        v.segments.get(going).map_or(v.end, |s| s.base)
    } else {
        next.map_or(v.start, |(i, _)| {
            v.segments.get(i + 1).map_or(v.end, |s| s.base)
        })
    };
    let pass_at_ms = if going > 0 {
        pass_after(r, v.now_ms, v.now_ms)
    } else {
        next.and_then(|(_, turns)| pass_after(r, turns, v.now_ms))
    };
    (going > 0 || next.is_some()).then_some(Expiry {
        going_now: going,
        new_start,
        next,
        pass_at_ms,
    })
}

fn bounds(r: &LogRetentionView) -> Vec<Span> {
    match (r.max_age_ms, r.max_bytes) {
        (Some(age), Some(size)) => vec![
            t("Segments older than "),
            Span::strong(span_ms(age)),
            t(
                " are deleted whole, oldest first, and so are segments that take the partition over ",
            ),
            Span::strong(bytes(size)),
            t(format!(", every {}.", every(r.every_ms))),
        ],
        (Some(age), None) => vec![
            t("Segments older than "),
            Span::strong(span_ms(age)),
            t(format!(
                " are deleted whole, oldest first, every {}.",
                every(r.every_ms)
            )),
        ],
        (None, Some(size)) => vec![
            t("Segments are deleted whole, oldest first, once the partition holds more than "),
            Span::strong(bytes(size)),
            t(format!(", every {}.", every(r.every_ms))),
        ],
        (None, None) => vec![t(
            "Nothing deletes records here: retention.ms and retention.bytes are not set, so the log is kept for ever.",
        )],
    }
}

/// The sentence under the name of a log that deletes.
#[must_use]
pub fn retention_sentence(v: &LogView) -> Vec<Span> {
    let Some(r) = &v.retention else {
        return Vec::new();
    };
    let mut out = bounds(r);
    let Some(e) = expiry(v) else {
        return out;
    };
    let at = |ms: Option<i64>| ms.map_or_else(|| "the first pass after that".to_owned(), clock_hm);
    if e.going_now > 0 {
        out.push(t(format!(
            " {} past it already and {} at the next pass",
            segments_word(e.going_now, false),
            if e.going_now == 1 { "goes" } else { "go" }
        )));
        out.push(t(format!(
            " ({}), which moves the log start from {} to {}.",
            at(e.pass_at_ms),
            number(v.start),
            number(e.new_start)
        )));
    } else if let (Some((i, turns)), Some(age)) = (e.next, r.max_age_ms) {
        out.push(t(format!(
            " Segment {} turns {} old at ",
            number_of(i),
            span_ms(age)
        )));
        out.push(Span::strong(clock_hm(turns)));
        out.push(t(" and goes at the pass at "));
        out.push(Span::strong(at(e.pass_at_ms)));
        let to = v.segments.get(i + 1).map_or(v.end, |s| s.base);
        out.push(t(format!(
            ", which moves the log start from {} to {}.",
            number(v.start),
            number(to)
        )));
    }
    out
}

fn clock_hm(ms: i64) -> String {
    clock(ms).chars().take(5).collect()
}

fn segments_word(n: usize, _cap: bool) -> String {
    if n == 1 {
        "Segment 1 is".to_owned()
    } else {
        format!("Segments 1 to {n} are")
    }
}

/// The rules of the next pass, with this partition's numbers.
#[must_use]
pub fn retention_rules(v: &LogView) -> Vec<Rule> {
    let Some(r) = &v.retention else {
        return Vec::new();
    };
    let ok = |title: String, sub: Vec<Span>, value: String| Rule {
        tone: RuleTone::Ok,
        title,
        sub,
        value,
    };
    let e = expiry(v);
    let age = match (r.max_age_ms, e.and_then(|e| e.next)) {
        (Some(age), next) => {
            let mut sub = vec![
                m("retention.ms"),
                t(format!(
                    " {}. A segment is as old as its newest record",
                    number(age)
                )),
            ];
            let value = match next {
                Some((i, turns)) => {
                    sub.push(t(format!(
                        ": segment {}, {}",
                        number_of(i),
                        date_time(turns)
                    )));
                    format!("turns {} at {}", span_ms(age), clock_hm(turns))
                }
                None => String::new(),
            };
            let past = r.would_delete > 0;
            ok(
                format!("Age: {}", span_ms(age)),
                sub,
                if past { "past it".to_owned() } else { value },
            )
        }
        (None, _) => ok(
            "Age: no limit".to_owned(),
            vec![m("retention.ms"), t(" -1")],
            "none".to_owned(),
        ),
    };
    let size = match r.max_bytes {
        Some(max) => ok(
            format!("Size: {}", bytes(max)),
            vec![
                m("retention.bytes"),
                t(format!(
                    " {}: the partition is left holding at least that; it holds {} now",
                    number(i64::try_from(max).unwrap_or(i64::MAX)),
                    bytes(v.bytes())
                )),
            ],
            if v.bytes() > max {
                "over".to_owned()
            } else {
                "under".to_owned()
            },
        ),
        None => ok(
            "Size: no limit".to_owned(),
            vec![m("retention.bytes"), t(" -1")],
            "none".to_owned(),
        ),
    };
    vec![policy(v), age, size, active_stays(v), checked(v, r)]
}

fn policy(v: &LogView) -> Rule {
    let both = v.compaction.is_some();
    Rule {
        tone: RuleTone::Ok,
        title: "The policy deletes".to_owned(),
        sub: vec![
            m("cleanup.policy"),
            t(if both {
                " is compact,delete: the log is cleaned, then old segments are deleted"
            } else {
                " is delete"
            }),
        ],
        value: if both { "compact,delete" } else { "delete" }.to_owned(),
    }
}

fn active_stays(v: &LogView) -> Rule {
    let n = v
        .segments
        .iter()
        .rposition(|s| s.active)
        .map_or(1, |i| i + 1);
    Rule {
        tone: RuleTone::Ok,
        title: "The active segment goes last".to_owned(),
        sub: vec![t(format!(
            "Segment {n} is deleted only when it is itself past retention, after a new one is started; an empty one never is"
        ))],
        value: "kept".to_owned(),
    }
}

fn checked(v: &LogView, r: &LogRetentionView) -> Rule {
    let look = every(r.every_ms);
    let (words, value) = match &r.last_pass {
        Some(p) => (
            format!(
                ": last pass {}, next {}",
                clock_hm(p.at_ms),
                clock_hm(p.at_ms + r.every_ms)
            ),
            clock_hm(p.at_ms + r.every_ms),
        ),
        None => (
            ": no pass has run since the broker started".to_owned(),
            format!("within {look}"),
        ),
    };
    let _ = v;
    Rule {
        tone: RuleTone::Ok,
        title: format!("Checked every {look}"),
        sub: vec![
            m(format!(
                "--log-retention-check-interval {}",
                flag(r.every_ms)
            )),
            t(words),
        ],
        value,
    }
}
