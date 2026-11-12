//! The retention side of a Log page: what the next pass will do, what the log is after its oldest segment goes, and what the
//! last pass did.

use maud::{Markup, html};

use super::planner::rule_list;
use crate::format::{clock, date_time, number, span_ms};
use crate::log_words::{expiry, number_of, retention_rules};
use crate::model::{LogRetentionView, LogView};
use crate::ui::button::{State, state};
use crate::ui::layout::{kv, value_with_sub};

fn hm(ms: i64) -> String {
    clock(ms).chars().take(5).collect()
}

/// `After segment 1 goes`: the log start moves, what a Fetch below it gets, what is kept.
fn after(v: &LogView) -> Markup {
    let Some(e) = expiry(v) else {
        return html! {};
    };
    let first = if e.going_now > 0 {
        e.going_now - 1
    } else {
        e.next.map_or(0, |n| n.0)
    };
    let below = format!("Fetch below {}", number(e.new_start));
    let word = if e.going_now > 1 {
        format!("After segments 1 to {} go", number_of(first))
    } else {
        format!("After segment {} goes", number_of(first))
    };
    html! {
        div class="card" {
            h2 { (word) }
            p class="lede" { "The new start is written down before any file is removed." }
            (kv(vec![
                ("Log start", html! { span class="mono" { (number(v.start)) " to " (number(e.new_start)) } }),
                (below.as_str(), html! { "OFFSET_OUT_OF_RANGE (1)" }),
                ("Kept", html! { span class="mono" { (number((v.end - e.new_start).max(0))) " offsets" } }),
            ]))
        }
    }
}

fn pass_card(v: &LogView, r: &LogRetentionView) -> Markup {
    let word = match &r.last_pass {
        None => state(State::Idle, "No pass yet"),
        Some(p) if p.moved.is_empty() && p.more == 0 => state(State::Idle, "Deleted nothing"),
        Some(_) => state(State::Ok, "Deleted"),
    };
    let mine: Vec<_> = r
        .last_pass
        .iter()
        .flat_map(|p| p.moved.iter())
        .filter(|m| m.topic == v.topic)
        .collect();
    let looked = (0..v.partitions.min(8))
        .map(|p| format!("{}-{p}", v.topic))
        .collect::<Vec<_>>()
        .join(", ");
    html! {
        div class="card" {
            div class="row sb" { h2 { "Last pass" } (word) }
            p class="lede" {
                @if let Some(p) = &r.last_pass { (hm(p.at_ms)) ", on " span class="mono" { (v.topic) } " (" (v.partitions) @if v.partitions == 1 { " partition" } @else { " partitions" } ")." }
                @else { "No pass has run since the broker started." }
            }
            (kv(vec![
                ("Looked at", html! { span class="mono" { (looked) } @if let Some(p) = &r.last_pass { span class="sub" { (p.looked) " partitions in all, across every topic with a retention bound" } } }),
                ("Past retention", past(&mine, r)),
                ("Deleted", deleted(r)),
                ("Next pass", html! { span class="mono" { (r.last_pass.as_ref().map_or_else(|| format!("within {}", span_ms(r.every_ms)), |p| hm(p.at_ms + r.every_ms))) } }),
            ]))
            p class="help" style="margin:12px 0 0" { "When a pass deletes, this card lists each partition whose log start it moved. The broker keeps the last pass only, in memory." }
        }
    }
}

fn past(mine: &[&crate::model::LogMoved], r: &LogRetentionView) -> Markup {
    if mine.is_empty() {
        return html! { span class="mono" { "0 segments" } span class="sub" { "no sealed segment was past retention" } };
    }
    let _ = r;
    html! {
        @for m in mine { span class="mono" { (m.topic) "-" (m.partition) ": log start " (number(m.from)) " to " (number(m.to)) } br; }
    }
}

fn deleted(r: &LogRetentionView) -> Markup {
    if r.moved_total == 0 {
        return value_with_sub(
            html! { "Nothing since the broker started" },
            "retention keeps no record before that",
        );
    }
    value_with_sub(
        html! { (number(i64::try_from(r.moved_total).unwrap_or(0))) " log starts moved since the broker started" },
        &r.moved_at_ms
            .map_or_else(String::new, |t| format!("the last at {}", date_time(t))),
    )
}

/// What the next pass will do, what follows it, and the last pass.
pub(super) fn cards(v: &LogView) -> Markup {
    let Some(r) = &v.retention else {
        return html! {};
    };
    html! {
        div class="split wide" {
            div class="card" data-extra="planner" {
                h2 { "What the next pass will do" }
                p class="lede" { "Retention reads " span class="mono" { "retention.ms" } " and " span class="mono" { "retention.bytes" } " of the topic, else the broker's." }
                (rule_list(&retention_rules(v)))
            }
            div class="stack-y" { (after(v)) (pass_card(v, r)) }
        }
    }
}
