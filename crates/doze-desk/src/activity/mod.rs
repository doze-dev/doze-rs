//! Activity (Go `view/activity.go`, `timeline.go`): what a stack did over the last hour, day or week, read from its
//! journal on disk. How long each service slept while the stack was up, and the memory that left free; what woke each
//! one and how often; what crashed. It answers "is doze saving me anything, and what keeps waking my stack".
//! Positions are fractions of the span, so the page draws without knowing about time.
//!
//! The figures are Go's, word for word (`tests/activity.rs` holds them to Go's own tests and to JSON Go wrote for the
//! same journal). The replay is linear in events plus services, where Go's walked every service at every event: a
//! 2,000-service stack with a full journal lays out in a few milliseconds.
//!
//! Beyond Go, kept out of the JSON: each stretch's times and wake-to-ready ([`SpanBar::took`]) for the page's hover,
//! and each row's typical wake-to-ready ([`Row::ready`]).

mod load;
mod replay;
mod words;

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::overview::Look;
use crate::page::time::Zone;
use crate::view::StackView;

pub use load::{Ev, events_of};
pub use words::{cause_word, gb_hours, hours};

/// The spans the page offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Span {
    Hour,
    #[default]
    Day,
    Week,
}

impl Span {
    /// Every span, in the page's order.
    pub const ALL: [Span; 3] = [Span::Hour, Span::Day, Span::Week];

    /// `1h`, `24h`, `7d`: Go's names.
    pub fn name(self) -> &'static str {
        match self {
            Span::Hour => "1h",
            Span::Day => "24h",
            Span::Week => "7d",
        }
    }

    /// The span named (Go: an unknown name is a day).
    pub fn parse(name: &str) -> Span {
        Span::ALL
            .into_iter()
            .find(|s| s.name() == name)
            .unwrap_or_default()
    }

    /// How long it is.
    pub fn length(self) -> Duration {
        Duration::from_secs(match self {
            Span::Hour => 3600,
            Span::Day => 24 * 3600,
            Span::Week => 7 * 24 * 3600,
        })
    }

    /// The segmented control's words.
    pub fn label(self) -> &'static str {
        match self {
            Span::Hour => "Last hour",
            Span::Day => "24 hours",
            Span::Week => "7 days",
        }
    }

    /// The head's words: "the last hour", "the last 24 hours".
    pub fn phrase(self) -> &'static str {
        match self {
            Span::Hour => "the last hour",
            Span::Day => "the last 24 hours",
            Span::Week => "the last 7 days",
        }
    }
}

/// What a stack did over a span (Go `view.Activity`).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Activity {
    pub key: String,
    pub stack: String,
    /// `1h`, `24h` or `7d`.
    pub span: &'static str,
    /// One service alone, or empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub inst: String,
    /// The axis of each row's strip.
    pub ticks: Vec<Tick>,
    pub summary: Summary,
    pub rows: Vec<Row>,
    /// Crashes, failed starts and give-ups, newest first.
    pub trouble: Vec<Line>,
    /// The journal's lines in the span, newest first, at most 150.
    pub events: Vec<Line>,
    /// The journal starts inside the span, so the figures cover less than the span says.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub short: String,
    /// Nothing happened in the span.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub empty: bool,
}

/// The stack's line at the top.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// "21h of 24h".
    pub up: String,
    /// The share of service-time asleep while up: "71%", or "—".
    pub asleep: String,
    /// "about 3.2 GB·h".
    #[serde(skip_serializing_if = "String::is_empty")]
    pub freed: String,
    /// What the estimate rests on.
    #[serde(rename = "freedNote", skip_serializing_if = "String::is_empty")]
    pub freed_note: String,
    pub wakes: usize,
    pub crashes: usize,
}

/// One service over the span (Go `ActivityRow`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    pub name: String,
    pub look: Look,
    /// The share asleep while the stack was up, 0..1.
    #[serde(rename = "asleepPart")]
    pub asleep_part: f64,
    /// "19h asleep · 79%", or "never slept".
    pub asleep: String,
    pub wakes: usize,
    /// What woke it, most often first: "9 for api · 5 by a connection".
    #[serde(rename = "wokenBy", skip_serializing_if = "String::is_empty")]
    pub woken_by: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub crashes: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub freed: String,
    /// Its strip: awake stretches and crashes.
    pub spans: Vec<SpanBar>,
    pub marks: Vec<Mark>,
    /// It never slept in the span (pinned, or always in use).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub never: bool,
    /// Beyond Go: its typical wake-to-ready in the span ("ready in 180ms, typically"), empty when it never got ready.
    #[serde(skip)]
    pub ready: String,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// An axis mark.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tick {
    pub at: f64,
    pub label: String,
}

/// A stretch it was awake: from its wake (or the span's edge) to its sleep, crash or now; waking is the part up to
/// ready (Go `TimelineSpanBar`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpanBar {
    pub from: f64,
    pub to: f64,
    /// Where the start finished, when it did inside the span; 0 when not.
    #[serde(skip_serializing_if = "zero")]
    pub ready: f64,
    pub title: String,
    /// Still awake now.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub open: bool,
    /// Beyond Go: how long the start took, when it finished.
    #[serde(skip)]
    pub took: Option<Duration>,
}

fn zero(f: &f64) -> bool {
    *f == 0.0
}

/// A moment: a crash, a failed start, a give-up (Go `TimelineMark`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Mark {
    pub at: f64,
    pub kind: String,
    pub title: String,
}

/// A line of the lists (Go `Recent`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Line {
    pub at: String,
    pub kind: String,
    pub text: String,
    pub tone: &'static str,
}

/// What [`activity_of`] reads besides the journal.
#[derive(Debug, Clone, Copy)]
pub struct Ask<'a> {
    pub span: Span,
    /// One service alone, or empty for all.
    pub inst: &'a str,
    /// Each service's last known memory in bytes, by name, for what sleeping left free.
    pub ram: &'a HashMap<String, i64>,
    pub now: SystemTime,
    pub zone: Zone,
}

/// Lays out a stack's journal over the span ending now (Go `ActivityOf`). `events` need not be sorted.
pub fn activity_of(st: &StackView, mut events: Vec<Ev>, ask: Ask<'_>) -> Activity {
    events.sort_by_key(|e| e.time);
    let name = if st.name.is_empty() {
        st.dir
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned()
    } else {
        st.name.clone()
    };
    let from = ask.now.checked_sub(ask.span.length()).unwrap_or(ask.now);
    let mut out = Activity {
        key: st.key.dir().to_string_lossy().into_owned(),
        stack: name,
        span: ask.span.name(),
        inst: ask.inst.to_owned(),
        ticks: words::ticks(from, ask.span, ask.zone),
        ..Activity::default()
    };
    if let Some(first) = events.first().filter(|e| e.time > from) {
        out.short = format!(
            "the journal goes back {}; the figures cover that",
            doze_display::ago(ask.now.duration_since(first.time).unwrap_or_default())
        );
    }
    let r = replay::replay(st, &events, from, &ask, &mut out);
    replay::rows(&r, &ask, &mut out);
    out
}
