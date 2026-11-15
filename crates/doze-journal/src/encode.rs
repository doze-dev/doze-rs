//! An event as one JSON line, byte for byte as Go's `json.Marshal` writes the Go `journal.Event`: the fields in its
//! order, `cause` left out when both of its fields are empty, `took` and `detail` when zero or empty, the time as
//! `RFC3339Nano` (Go's zero time when there is none), strings escaped as Go escapes them (`<`, `>`, `&`, U+2028 and
//! U+2029 included).

use crate::Event;
use std::fmt::Write;
use time::OffsetDateTime;

/// `e` as one line, without its newline.
pub fn encode(e: &Event) -> String {
    let mut out = String::with_capacity(160);
    let _ = write!(out, "{{\"seq\":{},\"time\":\"", e.seq);
    match e.time {
        Some(t) => rfc3339_nano(&mut out, t),
        None => out.push_str("0001-01-01T00:00:00Z"),
    }
    out.push_str("\",\"kind\":");
    string(&mut out, &e.kind);
    out.push_str(",\"instance\":");
    string(&mut out, &e.instance);
    if !e.cause.kind.is_empty() || !e.cause.by.is_empty() {
        out.push_str(",\"cause\":{");
        let mut first = true;
        for (k, v) in [("kind", &e.cause.kind), ("by", &e.cause.by)] {
            if v.is_empty() {
                continue;
            }
            if !first {
                out.push(',');
            }
            first = false;
            let _ = write!(out, "\"{k}\":");
            string(&mut out, v);
        }
        out.push('}');
    }
    if e.took_ns != 0 {
        let _ = write!(out, ",\"took\":{}", e.took_ns);
    }
    if !e.detail.is_empty() {
        out.push_str(",\"detail\":");
        string(&mut out, &e.detail);
    }
    out.push('}');
    out
}

/// Go's `time.RFC3339Nano`: the fraction with its trailing zeros dropped, `Z` for UTC, else `±hh:mm`.
/// `t` as Go's `time.RFC3339Nano` writes it (what Go's JSON holds for a `time.Time`).
pub fn go_time(t: OffsetDateTime) -> String {
    let mut out = String::with_capacity(36);
    rfc3339_nano(&mut out, t);
    out
}

fn rfc3339_nano(out: &mut String, t: OffsetDateTime) {
    let _ = write!(
        out,
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    );
    let nanos = t.nanosecond();
    if nanos != 0 {
        let frac = format!("{nanos:09}");
        out.push('.');
        out.push_str(frac.trim_end_matches('0'));
    }
    let off = t.offset();
    if off.is_utc() {
        out.push('Z');
    } else {
        let (h, m, _) = off.as_hms();
        let sign = if off.is_negative() { '-' } else { '+' };
        let _ = write!(out, "{sign}{:02}:{:02}", h.unsigned_abs(), m.unsigned_abs());
    }
}

/// A JSON string as Go's encoder writes it, HTML-safe.
fn string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '<' | '>' | '&' | '\u{2028}' | '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
