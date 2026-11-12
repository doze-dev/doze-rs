//! How the log is kept on disk: the Durability card, and the Segments table with a row per segment that opens to its files.

use maud::{Markup, html};

use super::planner::deletions_card;
use crate::base::Base;
use crate::format::{bytes, date_time, number};
use crate::log_words::pct;
use crate::model::{LogSegment, LogStop, LogView};
use crate::ui::button::{State, state};
use crate::ui::glyph_log::{LogGlyph, log_glyph};
use crate::ui::layout::{kv, value_with_sub};

/// The most segments listed: the newest ones, so that a log of thousands does not make a page of thousands of rows.
const MOST_ROWS: usize = 200;

const GRID: &str = "display:grid;grid-template-columns:130px 200px 100px 150px minmax(0,1fr) 20px;gap:16px;align-items:center;";

fn durability(v: &LogView, base: &Base) -> Markup {
    let d = &v.durability;
    let sealed = v.segments.len().saturating_sub(1);
    let active = v.segments.iter().rposition(|s| s.active);
    let (word, lede) = match &d.offline {
        Some(why) => (
            state(State::Bad, "Offline"),
            format!(
                "A flush failed: {why}. Appends are refused until the broker restarts; reads continue."
            ),
        ),
        None => (
            state(State::Ok, "Online"),
            format!(
                "An append that asks to be durable (acks=all) is acknowledged after its flush; the others are flushed within {} of the first. {}",
                crate::format::millis(u64::try_from(d.fsync_ms.max(1)).unwrap_or(1)),
                stop_sentence(d.stop)
            ),
        ),
    };
    let topic = base.url(&format!("/topics/{}", crate::base::segment(&v.topic)));
    html! {
        div class="card" {
            div class="row sb" { h2 { "Durability" } (word) }
            p class="lede" { (lede) }
            (kv(vec![
                ("Active log", value_with_sub(html! { span class="mono" { "segment " (active.map_or(1, |i| i + 1)) } }, &format!("from {}; flushed when an append asks, and on a timer for the rest", number(v.segments.last().map_or(v.start, |s| s.base))))),
                ("Its index, the directory", value_with_sub(html! { "Kept with the log" }, "the index as the log grows; the directory holds the segment file names")),
                ("Sealed segments", value_with_sub(html! { @if sealed == 0 { "none yet" } @else { span class="mono" { "1 to " (sealed) } } }, "each flushed whole, with its name, when it was sealed")),
                ("Last stop", value_with_sub(stop_word(d.stop), stop_note(d.stop))),
                ("Vouched for", value_with_sub(html! { (if d.stop == LogStop::Clean { "Yes" } else { "No" }) }, vouched_note(d.stop))),
            ]))
            p class="help" style="margin:12px 0 0" { "A failed flush is never retried: the partition goes offline until restart, appends are refused and reads continue. " a class="lnk" href=(topic) { "What the offline state looks like" } "." }
        }
    }
}

fn stop_word(s: LogStop) -> Markup {
    match s {
        LogStop::Clean => state(State::Ok, "Clean"),
        LogStop::Unclean => state(State::Warn, "Not clean"),
        LogStop::None => state(State::Idle, "None"),
    }
}

fn stop_sentence(s: LogStop) -> &'static str {
    match s {
        LogStop::Clean => "The last stop was clean, so this partition was opened as vouched for.",
        LogStop::Unclean => {
            "The last stop did not vouch for the data, so this partition was checked when it was opened."
        }
        LogStop::None => "This is a new data directory: there was no earlier stop.",
    }
}

fn stop_note(s: LogStop) -> &'static str {
    match s {
        LogStop::Clean => "every open partition was flushed and closed",
        LogStop::Unclean => "a crash, a kill or a power cut: nothing was vouched for",
        LogStop::None => "there was no earlier run",
    }
}

fn vouched_note(s: LogStop) -> &'static str {
    if s == LogStop::Clean {
        "only the stretch after the last index entry was checked; if it were not, the whole active segment is read and every checksum verified"
    } else {
        "the whole active segment was read and every checksum verified when the partition was opened"
    }
}

fn state_cell(v: &LogView, s: &LogSegment) -> (State, &'static str, String) {
    let Some(c) = &v.compaction else {
        return if s.active {
            (
                State::Info,
                "active",
                "deleted last, once past retention itself".to_owned(),
            )
        } else {
            (State::Idle, "sealed", String::new())
        };
    };
    if s.active {
        return (State::Info, "active", "never cleaned".to_owned());
    }
    if s.base < c.planner.dirty_start {
        let kept = if s.removed() > 0 && s.span() > 0 {
            format!(
                "kept {} of its offsets",
                pct(1.0 - s.removed() as f64 / s.span() as f64)
            )
        } else {
            "nothing to remove".to_owned()
        };
        return (State::Ok, "cleaned", kept);
    }
    let holds = if s.base <= c.planner.first_dirty && c.planner.first_dirty < s.next {
        "holds the checkpoint".to_owned()
    } else {
        String::new()
    };
    (State::Warn, "dirty", holds)
}

fn row(v: &LogView, s: &LogSegment, n: usize, open: bool) -> Markup {
    let (tone, word, sub) = state_cell(v, s);
    let last = if s.span() > 0 { s.next - 1 } else { s.base };
    let newest = s.newest_ms.map_or_else(
        || "none".to_owned(),
        |t| {
            if s.active && v.now_ms - t < 60_000 {
                "just now".to_owned()
            } else {
                date_time(t)
            }
        },
    );
    html! {
        details open[open] style="border-bottom:1px solid var(--line)" {
            summary style={(GRID) "padding:8px 16px;min-height:56px;cursor:pointer"} {
                span class="mono" style="font-weight:500;color:var(--bright)" { "Segment " (n) }
                span class="mono" { (number(s.base)) " to " (number(s.next)) }
                span class="mono" { (bytes(s.bytes)) }
                span class="mono" data-vary="" { (newest) }
                span { (state(tone, word)) @if !sub.is_empty() { span class="sub" style="display:block;font-size:12.5px;color:var(--dim)" { (sub) } } }
                span class="dim" { (log_glyph(LogGlyph::ChevD, 16, "")) }
            }
            div style="padding:0 16px 20px 16px" {
                (detail(v, s, last, &newest))
            }
        }
    }
}

fn detail(v: &LogView, s: &LogSegment, last: i64, newest: &str) -> Markup {
    let name = format!("{:020}", s.base);
    let entries = (s.bytes / v.index_interval_bytes.max(1)).max(1);
    let size_sub = if s.active {
        format!(
            "of segment.bytes {}; rolls when full",
            number(i64::try_from(v.segment_bytes).unwrap_or(0))
        )
    } else if s.removed() > 0 {
        format!(
            "smaller than {} because cleaning removed records",
            bytes(v.segment_bytes)
        )
    } else {
        format!(
            "segment.bytes {}",
            number(i64::try_from(v.segment_bytes).unwrap_or(0))
        )
    };
    let offsets_sub = if s.active {
        format!(
            "{} offsets so far; the log ends at {}",
            number(s.span()),
            number(v.end)
        )
    } else {
        format!(
            "{} offsets; the next segment starts at {}",
            number(s.span()),
            number(s.next)
        )
    };
    let mut rows = vec![
        (
            "Offsets",
            value_with_sub(
                html! { span class="mono" { (number(s.base)) " to " (number(last)) } },
                &offsets_sub,
            ),
        ),
        (
            "Size",
            value_with_sub(html! { span class="mono" { (bytes(s.bytes)) } }, &size_sub),
        ),
        ("Newest record", html! { span class="mono" { (newest) } }),
    ];
    if let Some(c) = &v.compaction {
        let (yes, why) = if s.active {
            ("Never", "the active segment is left alone".to_owned())
        } else if s.base < c.planner.dirty_start {
            (
                "Yes",
                if s.removed() > 0 && s.span() > 0 {
                    format!(
                        "kept {} of its offsets",
                        pct(1.0 - s.removed() as f64 / s.span() as f64)
                    )
                } else {
                    "nothing was left to remove".to_owned()
                },
            )
        } else {
            ("Not yet", "no round has looked at its records".to_owned())
        };
        rows.push(("Cleaned", value_with_sub(html! { (yes) }, &why)));
    }
    rows.push((
        "Files",
        html! { span class="mono" { (name) ".log" } span class="sub mono" { (name) ".index" } },
    ));
    rows.push((
        "Index",
        value_with_sub(
            html! { "about " (number(i64::try_from(entries).unwrap_or(0))) " entries" },
            &format!(
                "one for roughly every index.interval.bytes {} bytes",
                number(i64::try_from(v.index_interval_bytes).unwrap_or(0))
            ),
        ),
    ));
    html! {
        div style="border-top:1px solid var(--line)" { (kv(rows)) }
    }
}

fn segments(v: &LogView) -> Markup {
    let first_dirty = v.compaction.as_ref().and_then(|c| {
        v.segments.iter().position(|s| {
            !s.active && s.base <= c.planner.first_dirty && c.planner.first_dirty < s.next
        })
    });
    let skip = v.segments.len().saturating_sub(MOST_ROWS);
    html! {
        div class="sec" style="margin-top:40px" {
            h2 { "Segments" }
            p class="lede" {
                (v.segments.len()) @if v.segments.len() == 1 { " segment" } @else { " segments" } " of " span class="mono" { "segment.bytes" } " " (number(i64::try_from(v.segment_bytes).unwrap_or(0))) " (" (bytes(v.segment_bytes)) "). "
                @if v.compaction.is_some() { "Sealed segments are cleaned at most once per round; the active one never. " }
                "Open a row for its files and index."
                @if v.partial { " The log is large: some segments were read in part, and what is missing is not guessed." }
                @if skip > 0 { " The newest " (MOST_ROWS) " are listed." }
            }
            div style="background:#fff;border:1px solid var(--line);border-radius:14px;overflow:hidden" {
                div style={(GRID) "padding:8px 16px;height:44px;background:var(--well);border-bottom:1px solid var(--line);font-size:12px;font-weight:500;letter-spacing:.07em;text-transform:uppercase;color:var(--dim)"} {
                    span { "Segment" } span { "Offsets" } span { "Size" } span { "Newest record" } span { "State" } span {}
                }
                @for (i, s) in v.segments.iter().enumerate().skip(skip) { (row(v, s, i + 1, first_dirty == Some(i))) }
            }
        }
    }
}

/// The deletions and the durability, side by side, then the segments.
pub(super) fn durability_and_segments(v: &LogView, base: &Base) -> Markup {
    html! {
        div class="g2" style="margin-top:24px" {
            @if v.compaction.is_some() { (deletions_card(v, base)) }
            (durability(v, base))
        }
        (segments(v))
    }
}
