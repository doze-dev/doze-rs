//! The planner's rules as sentences: each rule of `Will the cleaner take it?` with this partition's numbers, the sentence
//! of what the next round will do, and what would make a log that is not due, due.

use super::{dirty_segments, every, flag, next_look_ms, number_of, pct, ratio_text};
use crate::format::{bytes, number, span_ms};
use crate::model::{LogCleaner, LogCompaction, LogLimit, LogView, Span};

/// Whether a rule lets the cleaner go on (`ok`, a check) or stops it (`no`, a warning).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "LogRuleTone"))]
#[serde(rename_all = "snake_case")]
pub enum RuleTone {
    Ok,
    No,
}

/// One rule: what it says, the numbers behind it, and its value at the right.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "LogRule"))]
pub struct Rule {
    pub tone: RuleTone,
    pub title: String,
    pub sub: Vec<Span>,
    pub value: String,
}

fn rule(tone: RuleTone, title: &str, sub: Vec<Span>, value: impl Into<String>) -> Rule {
    Rule {
        tone,
        title: title.to_owned(),
        sub,
        value: value.into(),
    }
}

fn t(s: impl Into<String>) -> Span {
    Span::text(s)
}

fn m(s: impl Into<String>) -> Span {
    Span::mono(s)
}

fn policy(v: &LogView) -> Rule {
    let both = v.retention.is_some();
    rule(
        RuleTone::Ok,
        "The policy compacts",
        vec![
            m("cleanup.policy"),
            t(if both {
                " is compact,delete: the log is cleaned, then retention applies"
            } else {
                " is compact; a topic that only deletes is never cleaned"
            }),
        ],
        if both { "compact,delete" } else { "compact" },
    )
}

fn share(v: &LogView, c: &LogCompaction) -> Rule {
    let p = &c.planner;
    let sealed = p.dirty_bytes + p.clean_bytes;
    let needs = vec![
        t(" needs more than "),
        m("min.cleanable.dirty.ratio"),
        t(format!(" {}", ratio_text(c.rules.min_dirty_ratio))),
    ];
    let _ = v;
    match p.ratio() {
        Some(r) => {
            let over = r > c.rules.min_dirty_ratio;
            let mut sub = vec![t(format!(
                "{} dirty of {} cleanable (clean {});",
                bytes(p.dirty_bytes),
                bytes(sealed),
                bytes(p.clean_bytes)
            ))];
            sub.extend(needs);
            rule(
                if over || p.waited_too_long {
                    RuleTone::Ok
                } else {
                    RuleTone::No
                },
                if over {
                    "The dirty share is over the threshold"
                } else {
                    "The dirty share is not over the threshold"
                },
                sub,
                format!(
                    "{} {} {}",
                    pct(r),
                    if over { ">" } else { "\u{2264}" },
                    pct(c.rules.min_dirty_ratio)
                ),
            )
        }
        None => rule(
            RuleTone::No,
            "Nothing is dirty",
            vec![t(
                "Every sealed segment below where cleaning stops has been cleaned; the share needs data to judge.",
            )],
            "0 B dirty",
        ),
    }
}

fn too_new(c: &LogCompaction) -> Rule {
    let lag = c.rules.min_lag_ms;
    if c.planner.limit == LogLimit::TooNew {
        return rule(
            RuleTone::No,
            "Some dirty segments are too new to clean",
            vec![
                m("min.compaction.lag.ms"),
                t(format!(
                    " is {}: a segment whose newest record is younger than that waits, and cleaning stops at its first offset {}",
                    span_ms(lag),
                    number(c.planner.upto)
                )),
            ],
            span_ms(lag),
        );
    }
    let sub = if lag == 0 {
        vec![
            m("min.compaction.lag.ms"),
            t(" is 0, so every sealed segment counts"),
        ]
    } else {
        vec![
            m("min.compaction.lag.ms"),
            t(format!(
                " is {}: every dirty segment is older",
                span_ms(lag)
            )),
        ]
    };
    rule(
        RuleTone::Ok,
        "Nothing is too new to clean",
        sub,
        if lag == 0 {
            "0 ms".to_owned()
        } else {
            span_ms(lag)
        },
    )
}

fn transaction(v: &LogView, c: &LogCompaction) -> Rule {
    if v.last_stable < v.end {
        return rule(
            RuleTone::No,
            "An open transaction holds cleaning back",
            vec![t(format!(
                "The last stable offset is {}, {} below the high watermark {}. Cleaning stops at the transaction's first offset",
                number(v.last_stable),
                number(v.end - v.last_stable),
                number(v.end)
            ))],
            format!("LSO {}", number(v.last_stable)),
        );
    }
    let _ = c;
    rule(
        RuleTone::Ok,
        "An open transaction below the last stable offset: none today",
        vec![t(
            "The last stable offset equals the high watermark. If a transaction were open, cleaning would stop at its first offset",
        )],
        format!("LSO {}", number(v.last_stable)),
    )
}

fn age(c: &LogCompaction) -> Rule {
    let Some(max) = c.rules.max_lag_ms else {
        return rule(
            RuleTone::Ok,
            "Not forced by age",
            vec![
                m("max.compaction.lag.ms"),
                t(" is not set, so only the share decides"),
            ],
            "no limit",
        );
    };
    if c.planner.waited_too_long {
        return rule(
            RuleTone::Ok,
            "Forced by age",
            vec![
                t("The oldest dirty record is older than "),
                m("max.compaction.lag.ms"),
                t(format!(
                    " ({}): the log is cleaned whatever the share",
                    span_ms(max)
                )),
            ],
            "overdue",
        );
    }
    rule(
        RuleTone::Ok,
        "Not yet forced by age",
        vec![
            t("The oldest dirty record is younger than "),
            m("max.compaction.lag.ms"),
            t(format!(" ({})", span_ms(max))),
        ],
        span_ms(max),
    )
}

fn active(v: &LogView) -> Rule {
    let Some(a) = v
        .segments
        .iter()
        .rposition(|s| s.active)
        .and_then(|i| v.segments.get(i).map(|s| (i, s)))
    else {
        return rule(
            RuleTone::Ok,
            "The active segment is left alone",
            vec![t("It is never cleaned")],
            "kept",
        );
    };
    rule(
        RuleTone::Ok,
        "The active segment is left alone",
        vec![t(format!(
            "Segment {} (from {}) is never cleaned; cleaning stops at its first offset",
            number_of(a.0),
            number(a.1.base)
        ))],
        format!("up to {}", number(a.1.base)),
    )
}

fn interval(v: &LogView, c: &LogCompaction) -> Rule {
    let look = every(c.every_ms);
    let next = match next_look_ms(c, v.now_ms) {
        Some(ms) => format!("the next look is in {}", span_ms(ms)),
        None => "it has not looked yet since the broker began".to_owned(),
    };
    match &c.cleaner {
        LogCleaner::Disabled => rule(
            RuleTone::No,
            "The cleaner is switched off",
            vec![
                m("--log-cleaner-enable false"),
                t(": no log is ever cleaned"),
            ],
            "off",
        ),
        LogCleaner::Failed { message, .. } => rule(
            RuleTone::No,
            "The last round failed",
            vec![t(message.clone())],
            "failed",
        ),
        LogCleaner::Running { .. } => rule(
            RuleTone::Ok,
            "A round is running now",
            vec![t("It is put in place a segment at a time")],
            "running",
        ),
        LogCleaner::Watching => rule(
            RuleTone::Ok,
            &format!("Looked at every {look}"),
            vec![
                m(format!("--log-cleaner-interval {}", flag(c.every_ms))),
                t(format!("; {next}")),
            ],
            if c.planner.due { "due" } else { "waiting" },
        ),
    }
}

/// The rules in the order the planner reads them.
#[must_use]
pub fn rules(v: &LogView) -> Vec<Rule> {
    let Some(c) = &v.compaction else {
        return Vec::new();
    };
    vec![
        policy(v),
        share(v, c),
        too_new(c),
        transaction(v, c),
        age(c),
        active(v),
        interval(v, c),
    ]
}

/// `A round will rewrite segments 5 to 7 (24.0 MiB) in place, ...`: what the next round does, for a log that is due.
#[must_use]
pub fn would_rewrite(v: &LogView) -> Vec<Span> {
    let Some(c) = &v.compaction else {
        return Vec::new();
    };
    let dirty = dirty_segments(v, c);
    let (Some(first), Some(last)) = (dirty.first(), dirty.last()) else {
        return Vec::new();
    };
    let which = if first == last {
        format!("segment {}", number_of(*first))
    } else {
        format!("segments {} to {}", number_of(*first), number_of(*last))
    };
    vec![
        t("A round will rewrite "),
        Span::strong(which),
        t(format!(
            " ({}) in place, keep the latest record of each key, and move the checkpoint to ",
            bytes(c.planner.dirty_bytes)
        )),
        Span::strong(number(c.planner.upto)),
        t("."),
    ]
}

/// What a round did, as `Last round` says it: `Nov 9 21:31, 43 min ago, on inventory.levels-0.`
#[must_use]
pub fn round_sentence(v: &LogView, c: &LogCompaction) -> Option<String> {
    let r = c.last?;
    Some(format!(
        "{}, {} ago, on {}-{}.",
        crate::format::date_time(r.at_ms),
        span_ms(v.now_ms - r.at_ms),
        v.topic,
        v.partition
    ))
}

/// The dirty bytes the log needs to be due, and what is still to come: for a log that is not due.
#[must_use]
pub fn still_to_come(v: &LogView, c: &LogCompaction) -> (String, String, Vec<Span>) {
    let r = c.rules.min_dirty_ratio.clamp(0.0, 0.999);
    let clean = c.planner.clean_bytes;
    let needs = (r / (1.0 - r) * clean as f64).floor() as u64;
    let active = v.segments.iter().rev().find(|s| s.active);
    let coming = match active {
        Some(a) => vec![
            t(format!(
                "The active segment (from {}, {} so far) joins the dirty part when it rolls at ",
                number(a.base),
                bytes(a.bytes)
            )),
            m("segment.bytes"),
            t(format!(" {}.", bytes(v.segment_bytes))),
        ],
        None => vec![t("Nothing is waiting to join the dirty part.")],
    };
    (
        bytes(needs.saturating_add(1)),
        bytes(c.planner.dirty_bytes),
        coming,
    )
}
