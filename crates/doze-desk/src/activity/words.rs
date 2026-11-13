//! Activity's words: the axis, the times, the durations and the causes, as Go wrote them.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use super::Span;
use crate::page::time::{Zone, secs};

const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];

/// Seconds since the epoch in the zone.
fn local(t: SystemTime, zone: Zone) -> i64 {
    secs(t) + i64::from(zone.0)
}

/// `15:04`.
pub(crate) fn hm(t: SystemTime, zone: Zone) -> String {
    let s = local(t, zone).rem_euclid(86_400);
    format!("{:02}:{:02}", s / 3600, s / 60 % 60)
}

/// `Mon`.
pub(crate) fn weekday(t: SystemTime, zone: Zone) -> &'static str {
    let day = local(t, zone).div_euclid(86_400).rem_euclid(7);
    DAYS.get(usize::try_from(day).unwrap_or(0))
        .copied()
        .unwrap_or("")
}

/// An event's time as the span wants it: the time of day within a day, the day and time over a week (Go `stamp`).
pub(crate) fn stamp(t: SystemTime, span: Span, zone: Zone) -> String {
    if span == Span::Week {
        format!("{} {}", weekday(t, zone), hm(t, zone))
    } else {
        zone.clock(t)
    }
}

/// The axis: every 10 minutes for an hour, every 4 hours for a day, every day for a week (Go `activityTicks`).
pub(crate) fn ticks(from: SystemTime, span: Span, zone: Zone) -> Vec<super::Tick> {
    let (step, n) = match span {
        Span::Hour => (600, 6_u32),
        Span::Day => (4 * 3600, 6),
        Span::Week => (24 * 3600, 7),
    };
    (0..=n)
        .map(|i| {
            let t = from + Duration::from_secs(step * u64::from(i));
            let label = match span {
                _ if i == n => "now".to_owned(),
                Span::Week => weekday(t, zone).to_owned(),
                _ => hm(t, zone),
            };
            super::Tick {
                at: f64::from(i) / f64::from(n),
                label,
            }
        })
        .collect()
}

/// A duration as the rows say it: "45m", "19h", "3d 4h" (Go `hours`).
pub fn hours(d: Duration) -> String {
    let secs = d.as_secs_f64();
    let h = secs / 3600.0;
    if secs < 3600.0 {
        format!("{}m", d.as_secs() / 60)
    } else if h < 48.0 {
        format!("{}h", whole(h + 0.5))
    } else {
        let whole_h = whole(h);
        format!("{}d {}h", whole_h / 24, whole_h % 24)
    }
}

/// Go's `int(x)` for a positive float.
fn whole(x: f64) -> u64 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // WHY: Go's int() of a positive figure, which truncates
    let n = x.max(0.0) as u64;
    n
}

/// Memory over time: "1.4 GB·h", "320 MB·h" (Go `gbHours`).
pub fn gb_hours(gbh: f64) -> String {
    if gbh < 1.0 {
        format!("{} MB·h", whole(gbh * 1024.0 + 0.5))
    } else {
        format!("{gbh:.1} GB·h")
    }
}

/// Why a service woke, as the rows say it (Go `causeWord`).
pub fn cause_word(kind: &str, by: &str) -> String {
    match (kind, by) {
        ("watch", b) if !b.is_empty() => format!("for a change to {b}"),
        (_, b) if !b.is_empty() => format!("for {b}"),
        ("connection", _) => "by a connection".into(),
        ("command", _) => "when asked".into(),
        ("restart", _) => "to restart".into(),
        ("", _) => "on its own".into(),
        (k, _) => format!("by {k}"),
    }
}

/// The causes, most often first, three at most: "9 for api · 5 by a connection" (Go `causeList`).
pub(crate) fn cause_list(causes: &HashMap<String, usize>) -> String {
    let mut all: Vec<(&String, &usize)> = causes.iter().collect();
    all.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    all.iter()
        .take(3)
        .map(|(k, n)| format!("{n} {k}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// " · " and the detail's first line, cut at 80 bytes (Go `firstWords`).
pub(crate) fn first_words(detail: &str) -> String {
    let line = doze_display::first_line(detail);
    if line.is_empty() {
        return String::new();
    }
    if line.len() <= 80 {
        return format!(" · {line}");
    }
    let mut cut = 80;
    while !line.is_char_boundary(cut) {
        cut -= 1;
    }
    format!(" · {}…", line.get(..cut).unwrap_or_default())
}

/// The tone of an event's first word (Go `eventTone`).
pub(crate) fn event_tone(kind: &str) -> &'static str {
    match kind {
        "woke" => "accent",
        "ready" => "good",
        "idle" | "slept" => "warn",
        "crashed" | "failed" => "bad",
        _ => "dim",
    }
}

/// A wake-to-ready as the hover says it: "180ms", "1.2s".
pub(crate) fn took(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", d.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_and_memory_read_as_go_wrote_them() {
        assert_eq!(hours(Duration::from_secs(45 * 60 + 59)), "45m");
        assert_eq!(hours(Duration::from_secs(19 * 3600 + 1800)), "20h");
        assert_eq!(hours(Duration::from_secs(76 * 3600)), "3d 4h");
        assert_eq!(gb_hours(0.3125), "320 MB·h");
        assert_eq!(gb_hours(10.0), "10.0 GB·h");
        assert_eq!(cause_word("watch", "src/"), "for a change to src/");
        assert_eq!(cause_word("dependency", "api"), "for api");
        assert_eq!(cause_word("", ""), "on its own");
        assert_eq!(cause_word("timer", ""), "by timer");
        assert_eq!(first_words(&"é".repeat(60)).chars().last(), Some('…'));
    }

    #[test]
    fn the_week_reads_in_days() {
        // 2026-11-06 was a Friday.
        let t = std::time::UNIX_EPOCH + Duration::from_secs(20_763 * 86_400);
        assert_eq!(weekday(t, Zone::UTC), "Fri");
        assert_eq!(stamp(t, Span::Week, Zone::UTC), "Fri 00:00");
        assert_eq!(stamp(t, Span::Day, Zone(3600)), "01:00:00");
    }
}
