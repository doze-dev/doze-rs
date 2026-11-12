//! The cleaner's side of a Log page: the planner's rules with this partition's numbers, what would make a log that is not due,
//! due, the last round, and the deletions a round has kept.

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::format::{bytes, date_time, millis, number, span_ms};
use crate::log_words::{Rule, RuleTone, pct, round_sentence, rules, still_to_come, would_rewrite};
use crate::model::{LogCleaner, LogCompaction, LogView};
use crate::ui::button::{State, state};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::glyph_log::{LogGlyph, log_glyph};
use crate::ui::layout::{hr, kv, value_with_sub};
use crate::ui::notice::{Tone, callout};
use crate::ui::write::spans;

pub(super) fn rule_list(rules: &[Rule]) -> Markup {
    html! {
        div class="rules" {
            @for r in rules {
                div class={"r " @if r.tone == RuleTone::Ok { "ok" } @else { "no" }} {
                    @if r.tone == RuleTone::Ok { (glyph(Glyph::Check, 16, "")) } @else { (glyph(Glyph::Warn, 16, "")) }
                    div { (r.title) span class="sub" { (spans(&r.sub)) } }
                    span class="v" { (r.value) }
                }
            }
        }
    }
}

/// The word beside the planner card's title.
fn verdict(c: &LogCompaction) -> Markup {
    match &c.cleaner {
        LogCleaner::Disabled => state(State::Warn, "Cleaner off"),
        LogCleaner::Running { .. } => state(State::Info, "Cleaning now"),
        LogCleaner::Failed { .. } => state(State::Bad, "Last round failed"),
        LogCleaner::Watching if c.planner.due => state(State::Info, "Due at the next look"),
        LogCleaner::Watching => state(State::Warn, "Not due"),
    }
}

/// The callout above the ruler of a log that is not due.
pub(super) fn not_due_callout(v: &LogView) -> Markup {
    let Some(c) = &v.compaction else {
        return html! {};
    };
    let why = match c.planner.ratio() {
        Some(r) => format!(
            "Not yet: {} of the log is dirty, the cleaner waits for more than {}.",
            pct(r),
            pct(c.rules.min_dirty_ratio)
        ),
        None => "Not yet: nothing sealed is dirty.".to_owned(),
    };
    html! {
        div style="margin-bottom:24px" {
            (callout(Tone::Plain, Glyph::Info, html! { b { (why) } " The active segment is never cleaned." }))
        }
    }
}

fn planner_card(v: &LogView, c: &LogCompaction, title: &str, lede: Markup, base: &Base) -> Markup {
    let _ = base;
    html! {
        div class="card" data-extra="planner" {
            div class="row sb" { h2 { (title) } (verdict(c)) }
            p class="lede" { (lede) }
            (rule_list(&rules(v)))
            @if c.planner.due {
                (hr())
                p class="sent" { (spans(&would_rewrite(v))) }
            }
        }
    }
}

/// `Last round`.
fn last_round(v: &LogView, c: &LogCompaction) -> Markup {
    let Some(r) = c.last else {
        return html! {
            div class="card" {
                div class="row sb" { h2 { "Last round" } (state(State::Idle, "None yet")) }
                p class="lede" { "No round has run on " span class="mono" { (v.topic) "-" (v.partition) } " since the broker started." }
                p class="help" style="margin:12px 0 0" { "The broker keeps only the last round of each partition, in memory: it is forgotten when the broker stops." }
            }
        };
    };
    let kept = if r.read == 0 {
        0.0
    } else {
        r.kept as f64 / r.read as f64
    };
    let removed = r.read.saturating_sub(r.kept);
    let word = match &c.cleaner {
        LogCleaner::Failed { .. } => state(State::Bad, "Failed"),
        LogCleaner::Running { .. } => state(State::Info, "Running"),
        _ => state(State::Ok, "Done"),
    };
    html! {
        div class="card" {
            div class="row sb" { h2 { "Last round" } (word) }
            p class="lede" { (round_sentence(v, c).unwrap_or_default()) }
            div class="lagbar" style="margin-bottom:6px" { div class="tr" { i style={"--p:" (format!("{:.0}", kept * 100.0)) "%"} {} } span class="n" { (pct(kept)) " kept" } }
            p class="help" style="margin:0 0 12px" { (number(i64::try_from(r.kept).unwrap_or(0))) " of " (number(i64::try_from(r.read).unwrap_or(0))) " records stayed: the latest of each key." }
            (kv(vec![
                ("Read", html! { span class="mono" { (number(i64::try_from(r.read).unwrap_or(0))) " records" } }),
                ("Kept", html! { span class="mono" { (number(i64::try_from(r.kept).unwrap_or(0))) " (" (pct(kept)) ")" } }),
                ("Removed", value_with_mono(&number(i64::try_from(removed).unwrap_or(0)), "older values of a key that has a newer one")),
                ("Replaced", value_with_sub(html! { (r.replaced) @if r.replaced == 1 { " segment" } @else { " segments" } }, "written beside the log, then swapped in")),
                ("Took", html! { span class="mono" { (millis(r.took_ms)) } }),
                ("Checkpoint", value_with_mono(&number(r.clean_to), "the first dirty offset after the round")),
            ]))
            @if let LogCleaner::Failed { message, at_ms } = &c.cleaner {
                p class="help" style="margin:12px 0 0" { "A later round failed at " (date_time(*at_ms)) ": " (message) }
            }
            p class="help" style="margin:12px 0 0" { "The broker keeps only the last round of each partition, in memory: it is forgotten when the broker stops." }
        }
    }
}

fn value_with_mono(value: &str, sub: &str) -> Markup {
    html! { span class="mono" { (value) } span class="sub" { (sub) } }
}

fn failed_card(c: &LogCompaction) -> Markup {
    let LogCleaner::Failed { at_ms, message } = &c.cleaner else {
        return html! {};
    };
    html! {
        div class="card" {
            div class="row sb" { h2 { "Last round" } (state(State::Bad, "Failed")) }
            p class="lede" { "The last round failed at " (date_time(*at_ms)) ": " (message) }
        }
    }
}

/// A log that is due: the planner and the last round.
pub(super) fn due_cards(v: &LogView, base: &Base) -> Markup {
    let Some(c) = &v.compaction else {
        return html! {};
    };
    html! {
        div class="split wide" {
            (planner_card(v, c, "Will the cleaner take it?", html! { "The planner's rules, as Kafka's cleaner reads them (CMP-1), with this partition's numbers." }, base))
            div class="stack-y" { (last_round(v, c)) }
        }
    }
}

/// A log that is not due: why it waits, and what would make it due; the last round when there is one.
pub(super) fn not_due_cards(v: &LogView, base: &Base) -> Markup {
    let Some(c) = &v.compaction else {
        return html! {};
    };
    let (needs, has, coming) = still_to_come(v, c);
    let config = base.url(&format!("/topics/{}/config", segment(&v.topic)));
    let has_round = c.last.is_some();
    html! {
        div class="split wide" {
            (planner_card(v, c, "Why it waits", html! { "The same rules as for a log that is due, one of them failing." }, base))
            div class="stack-y" {
                div class="card" {
                    h2 { "What would make it due" }
                    p class="lede" { "The share counts sealed segments only." }
                    (kv(vec![
                        ("Needs", value_with_mono(&format!("dirty over {needs}"), &format!("more than {} already clean", bytes(c.planner.clean_bytes)))),
                        ("Has", html! { span class="mono" { (has) } }),
                        ("Still to come", html! { (spans(&coming)) }),
                    ]))
                    (hr())
                    p class="help" { "Lowering " span class="mono" { "min.cleanable.dirty.ratio" } " on this topic makes it due sooner. " a class="lnk" href=(config) { "See the topic's config" } "." }
                }
                @if has_round { (last_round(v, c)) } @else { (failed_card(c)) }
            }
        }
    }
}

/// The card of the deletions a round has kept.
pub(super) fn deletions_card(v: &LogView, base: &Base) -> Markup {
    let Some(c) = &v.compaction else {
        return html! {};
    };
    let record = |offset: i64| {
        base.url(&format!(
            "/topics/{}/messages/{}/{offset}",
            segment(&v.topic),
            v.partition
        ))
    };
    html! {
        div class="card" {
            h2 { "Deletions" }
            p class="lede" {
                "Tombstones a round has kept, with the time after which the next round removes them. A deletion stays for "
                span class="mono" { "delete.retention.ms" } " (" (span_ms(c.rules.delete_retention_ms)) "), so a slow reader still sees that the key was deleted."
            }
            table class="t" {
                thead { tr { th scope="col" { "Key" } th scope="col" { "Deleted" } th scope="col" { "Goes after" } } }
                tbody {
                    @for t in &v.tombstones {
                        tr {
                            td class="name mono" { (t.key) @if t.binary { " " span class="dim" { "(not text)" } } span class="sub" { "offset " (number(t.offset)) } }
                            td class="mono" { (date_time(t.written_ms)) }
                            td class="mono" data-vary="" { (date_time(t.goes_after_ms)) span class="sub" { @if t.goes_after_ms > v.now_ms { "in " (span_ms(t.goes_after_ms - v.now_ms)) } @else { "at the next round" } } }
                        }
                    }
                }
            }
            @if v.tombstones.is_empty() {
                p class="help" style="margin:12px 0 0" { "No deletion is waiting to go. Tombstones still in the dirty part have no horizon yet, so they are not listed." }
            } @else {
                p class="help" style="margin:12px 0 0" { (count_of(v.tombstones.len())) " waiting to go. Tombstones still in the dirty part have no horizon yet, so they are not listed." }
                @if let Some(t) = v.tombstones.first() {
                    div class="row" style="margin-top:16px" {
                        a class="btn sm" href=(record(t.offset)) data-op="Fetch" { (log_glyph(LogGlyph::Eye, 16, "")) "Read the tombstone" }
                        span class="help" { "A Fetch at offset " (number(t.offset)) " returns the record with no value." }
                    }
                }
            }
        }
    }
}

fn count_of(n: usize) -> String {
    if n == 1 {
        "1 tombstone is".to_owned()
    } else {
        format!("{} tombstones are", number(i64::try_from(n).unwrap_or(0)))
    }
}
