//! The Lag tab in its four states: `group-lag`, `lag-no-commits`, `lag-ahead` and `lag-expiring`. The data chooses, except
//! that the expiring view is asked for (`?expiring=1`).

use maud::{Markup, html};

use super::common::{On, group_tabs, group_url, head, spans, switch};
use super::detail::lagbar;
use crate::base::{Base, segment};
use crate::format::{ago, date_time, number, span_ms};
use crate::groups_words::lag_sentence;
use crate::lag::Lag;
use crate::model::{GroupHead, GroupLagView, LagRow, LagState, LagTopic};
use crate::ui::client;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::glyph_groups::{GroupGlyph, group_glyph};
use crate::ui::inplace;
use crate::ui::notice::{Tone, callout};

/// The region of the Lag tab: everything under the flash, which the state (`?expiring=1`) changes as a whole.
pub(super) const LAG_VIEW: &str = "group-lag-view";

/// The fragments of the Lag tab: the sentence and the table.
pub const LAG_TABLE: &str = "group-lag-table";

fn query(v: &GroupLagView) -> String {
    format!(
        "?group={}{}",
        segment(&v.head.id),
        if v.state == LagState::Expiring {
            "&expiring=1"
        } else {
            ""
        }
    )
}

fn when(h: &GroupHead, ms: i64) -> String {
    crate::format::stamp(ms, h.now_ms)
}

pub(crate) fn cell_lag(r: &LagRow) -> Markup {
    match r.lag {
        Lag::Behind(n) => html! { (lagbar(r.bar, Some(n))) },
        Lag::CaughtUp => html! { (lagbar(0, Some(0))) },
        Lag::PastEnd(by) => html! { span class="st warn" { (number(by)) " past the end" } },
        Lag::NoCommit => html! { span class="st idle" { "No commits yet" } },
        Lag::Gone => html! { span class="st bad" { "Topic gone" } },
    }
}

fn topic_head(t: &LagTopic, v: &GroupLagView, base: &Base, cols: usize) -> Markup {
    let mut bits: Vec<String> = vec![format!(
        "{} {}",
        t.partitions,
        if t.partitions == 1 {
            "partition"
        } else {
            "partitions"
        }
    )];
    if t.compacted {
        bits.push("compacted".to_owned());
    }
    if let Some(r) = t.retention_ms.filter(|r| *r >= 0) {
        bits.push(format!("retention {}", span_ms(r)));
    }
    match t.holders.as_slice() {
        [] if v.head.members.is_empty() => bits.push("no member holds any".to_owned()),
        [] => {}
        [one] => bits.push(format!("held by {one}")),
        many => bits.push(format!("held by {} members", many.len())),
    }
    if !t.exists {
        bits.push("the topic is gone".to_owned());
    }
    let committed = t.rows.iter().any(|r| r.committed.is_some());
    html! {
        tr id=(format!("lagh-{}", t.name)) { td colspan=(cols) style="background:var(--well);height:44px" {
            a class="lnk mono" href=(base.url(&format!("/topics/{}", segment(&t.name)))) { (t.name) }
            " " span class="dim" { "\u{a0}" (bits.join(", ")) }
            @if !committed { span class="mono bright" style="float:right" { "lag not known" } }
            @else if t.behind > 0 { span class="mono bright" style="float:right" { (number(t.behind)) " behind" } }
            @else if t.rows.iter().all(|r| r.committed.is_some()) { span class="mono bright" style="float:right" { "0 behind" } }
            @else { span class="mono bright" style="float:right" { (number(t.behind)) " behind" } }
        } }
    }
}

fn row(r: &LagRow, h: &GroupHead, ahead: bool) -> Markup {
    html! {
        tr id=(format!("lag-{}-{}", r.topic, r.partition)) class=[(ahead && matches!(r.lag, Lag::PastEnd(_))).then_some("sel")] {
            td class="name mono" { (r.topic) "-" (r.partition) }
            td class="r mono" { @match r.end { Some(e) => (number(e)), None => span class="dim" { "—" } } }
            @match r.committed {
                Some(c) => td class="r mono" { (number(c)) },
                None => td class="r dim" { "none" },
            }
            td { (cell_lag(r)) }
            @match r.last_commit_ms {
                Some(ms) => td class="mono" data-vary { (crate::ui::table::when(&when(h, ms), ms)) },
                None => td class="dim" { "never" },
            }
        }
    }
}

fn normal_table(v: &GroupLagView, base: &Base) -> Markup {
    html! {
        table class="t" data-extra="lag-line" {
            thead { tr {
                th scope="col" { "Partition" } th scope="col" class="r" { "Log end" } th scope="col" class="r" { "Committed" }
                th scope="col" { "Lag" } th scope="col" { "Last commit" }
            } }
            tbody {
                @for t in &v.topics {
                    (topic_head(t, v, base, 5))
                    @for r in &t.rows { (row(r, &v.head, v.state == LagState::Ahead)) }
                }
            }
        }
    }
}

fn expiring_table(v: &GroupLagView, base: &Base) -> Markup {
    html! {
        table class="t" data-extra="lag-line" {
            thead { tr {
                th scope="col" { "Partition" } th scope="col" class="r" { "Log end" } th scope="col" class="r" { "Committed" }
                th scope="col" { "Forgotten at" } th scope="col" { "Last commit" }
            } }
            tbody {
                @for t in v.topics.iter().filter(|t| t.rows.iter().any(|r| r.committed.is_some())) {
                    (topic_head(t, v, base, 5))
                    @for r in t.rows.iter().filter(|r| r.committed.is_some()) {
                        tr {
                            td class="name mono" { (r.topic) "-" (r.partition) }
                            td class="r mono" { @match r.end { Some(e) => (number(e)), None => span class="dim" { "—" } } }
                            td class="r mono" { (number(r.committed.unwrap_or(0))) }
                            td { @match r.expires_ms { Some(ms) => span class="mono" data-vary { (date_time(ms).trim_end_matches(" UTC")) }, None => span class="dim" { "not while things stay as they are" } } }
                            td class="mono" data-vary { (r.last_commit_ms.map_or_else(|| "never".to_owned(), |ms| date_time(ms).trim_end_matches(" UTC").to_owned())) }
                        }
                    }
                }
            }
        }
    }
}

/// The table, as a fragment.
#[must_use]
pub fn lag_table_fragment(v: &GroupLagView, base: &Base) -> Markup {
    client::fragment(
        LAG_TABLE,
        &base.url(&format!("/fragments/{LAG_TABLE}{}", query(v))),
        html! {
            @if v.state == LagState::Expiring { (expiring_table(v, base)) } @else { (normal_table(v, base)) }
        },
    )
}

fn acts(v: &GroupLagView, base: &Base) -> Markup {
    let h = &v.head;
    let reset = |pri: bool| {
        html! {
            a class={"btn" @if pri { " pri" }} href=(group_url(base, &h.id, "/reset")) data-dialog data-op="OffsetCommit" { (glyph(Glyph::Refresh, 16, "")) "Reset offsets" }
        }
    };
    let delete_offsets = html! {
        a class="btn" href=(group_url(base, &h.id, "/offsets/delete")) data-dialog data-op="OffsetDelete" { (glyph(Glyph::Trash, 16, "")) "Delete offsets" }
    };
    html! {
        @match v.state {
            LagState::Ahead => {
                (reset(true))
                (inplace::link(LAG_VIEW, "btn", &format!("{}?expiring=1", group_url(base, &h.id, "/lag")), None, html! { (group_glyph(GroupGlyph::Clock, 16, "")) "When do these expire?" }))
                a class="btn danger" href=(group_url(base, &h.id, "/delete")) data-dialog { (glyph(Glyph::Trash, 16, "")) "Delete group" }
            },
            LagState::Expiring => { (delete_offsets) (reset(false)) },
            _ => { (reset(false)) (delete_offsets) },
        }
    }
}

fn explain_no_commit(v: &GroupLagView) -> Markup {
    let first = v
        .topics
        .iter()
        .flat_map(|t| &t.rows)
        .find(|r| r.lag == Lag::NoCommit);
    let end = first
        .and_then(|r| r.end)
        .map_or_else(|| "its".to_owned(), number);
    html! {
        div style="margin-top:24px" data-extra="teaching" {
            (callout(Tone::Plain, Glyph::Info, html! {
                b { "No commit is not lag " (end) "." }
                " OffsetFetch answers offset -1 for a partition the group never committed, and the console shows that as \"none\". A consumer that finds -1 starts where its own "
                span class="mono" { "auto.offset.reset" } " says: " span class="mono" { "earliest" } " reads all " (end) " records, "
                span class="mono" { "latest" } " skips them. The broker cannot know which. The first commit from " (v.head.id) " turns this row into a number."
            }))
        }
    }
}

fn retention_text(v: &GroupLagView) -> (String, String) {
    let hours = v.retention_ms / 3_600_000;
    (
        span_ms(v.retention_ms),
        format!("--offsets-retention {hours}h"),
    )
}

fn explain_ahead(v: &GroupLagView, base: &Base) -> Markup {
    let h = &v.head;
    let first = v
        .topics
        .iter()
        .flat_map(|t| &t.rows)
        .find(|r| matches!(r.lag, Lag::PastEnd(_)));
    let (committed, end, id) = first.map_or((0, 0, String::new()), |r| {
        (
            r.committed.unwrap_or(0),
            r.end.unwrap_or(0),
            format!("{}-{}", r.topic, r.partition),
        )
    });
    let expires = v
        .topics
        .iter()
        .flat_map(|t| &t.rows)
        .filter_map(|r| r.expires_ms)
        .min();
    html! {
        @if h.members.is_empty() {
            div style="margin-top:24px" data-extra="teaching" {
                (callout(Tone::Plain, Glyph::Info, html! {
                    b { "Nobody is in this group." }
                    " The last member left, or its session ran out. The group is Empty, so it can be reset or deleted: Kafka only accepts those while no member is there. It still holds committed offsets"
                    @if let Some(t) = v.topics.iter().find(|t| t.rows.iter().any(|r| r.committed.is_some())) { " on " a class="lnk" href=(base.url(&format!("/topics/{}", segment(&t.name)))) { (t.name) } }
                    ", which are not forgotten until the retention date" @if let Some(at) = expires { ", in " (span_ms(at - h.now_ms)) } "."
                }))
            }
        }
        div class="g2" style="margin-top:24px" data-extra="teaching" {
            (callout(Tone::Warn, Glyph::Warn, html! {
                b { "Kafka accepted this commit." }
                " It stores offsets past the end without a check. The usual cause is a topic that was recreated while an app kept its old position, then committed it. Nothing is broken until a consumer starts."
            }))
            (callout(Tone::Plain, Glyph::Info, html! {
                b { "What a consumer would do." }
                " It asks to fetch " (number(committed)) " and is told OFFSET_OUT_OF_RANGE. It then follows its "
                span class="mono" { "auto.offset.reset" } ": " span class="mono" { "latest" } " skips ahead to " (number(end)) ", "
                span class="mono" { "earliest" } " re-reads " (id) " from its start. Resetting to latest here fixes it before that happens."
            }))
        }
    }
}

fn expiring_window(v: &GroupLagView) -> Markup {
    let h = &v.head;
    let first = v
        .topics
        .iter()
        .flat_map(|t| &t.rows)
        .filter_map(|r| r.expires_ms)
        .min();
    let (long, flag) = retention_text(v);
    let Some(at) = first else {
        return html! {
            div class="card" style="margin-bottom:32px" {
                h2 { "Retention window" }
                p class="help" style="max-width:none" {
                    (long) " (" span class="mono" { (flag) } "). No date is set while things stay as they are: "
                    @if h.members.is_empty() { "the group may yet have members." } @else { "a group with members keeps what it subscribes to." }
                }
            }
        };
    };
    let from = at - v.retention_ms - 1;
    let pct = u8::try_from(((h.now_ms - from) * 100 / v.retention_ms.max(1)).clamp(0, 100))
        .unwrap_or(100);
    let basis = if h.empty_since_ms == Some(from) {
        "when the group lost its last member"
    } else {
        "the commit"
    };
    html! {
        div class="card" style="margin-bottom:32px" {
            h2 { "Retention window" }
            p class="help" style="max-width:none;margin-bottom:16px" {
                (long) " (" span class="mono" { (flag) } "), counted from " (basis) ", " (date_time(from).trim_end_matches(" UTC")) "."
            }
            div class="lagbar" {
                span class="mono dim" { (date_time(from).trim_end_matches(" UTC")) }
                div class="tr" { i class="warn" style={"--p:" (pct) "%"} {} }
                span class="mono dim" style="min-width:0" { (date_time(at).trim_end_matches(" UTC")) }
            }
            div class="row sb" style="margin-top:10px" {
                span class="help" data-vary { (span_ms(h.now_ms - from)) " gone" }
                span class="help bright" data-vary { (span_ms(at - h.now_ms)) " left" }
            }
        }
    }
}

fn explain_expiring(v: &GroupLagView) -> Markup {
    html! {
        div class="g2" style="margin-top:24px" data-extra="teaching" {
            (callout(Tone::Plain, Glyph::Info, html! {
                b { "What happens then." }
                " At the first sweep after that time the offsets go. A group with no members and no offsets is forgotten, and the id is free: a consumer that joins as "
                (v.head.id) " afterwards is a new group and starts at its " span class="mono" { "auto.offset.reset" } "."
            }))
            (callout(Tone::Warn, Glyph::Warn, html! {
                b { "To keep them," } " start a consumer in " (v.head.id) " before then: a group with members keeps what it subscribes to."
            }))
        }
    }
}

/// The Lag tab, in the state its data calls for.
#[must_use]
pub fn lag_page(v: &GroupLagView, base: &Base) -> Markup {
    let h = &v.head;
    html! {
        @if let Some(f) = &v.flash {
            div class="banner" style="margin-top:24px" { (glyph(Glyph::Check, 18, "")) span { (f) } }
        }
        // The state is the data and the address (`?expiring=1`): asking when the offsets expire changes the buttons, the table and
        // what is said under it, in place, so the whole page body is the region.
        (inplace::region(LAG_VIEW, html! {
        (head(h, super::common::stats_fragment(h, base), acts(v, base)))
        (switch(h, base, "/lag"))
        (group_tabs(h, base, On::Lag))
        @if v.state == LagState::Expiring { (expiring_window(v)) }
        (lag_table_fragment(v, base))
        @match v.state {
            LagState::Normal => {
                @let behind = v.topics.iter().flat_map(|t| &t.rows).find(|r| matches!(r.lag, Lag::Behind(_)));
                p class="help" style="margin-top:16px;max-width:none" {
                    "A full bar would be a consumer that has read none of the partition. "
                    @if let Some(r) = behind { (r.topic) "-" (r.partition) " is " (r.bar) " % of its log behind. " }
                    "The broker keeps no lag history: this is the lag now."
                }
            },
            LagState::NoCommits => (explain_no_commit(v)),
            LagState::Ahead => (explain_ahead(v, base)),
            LagState::Expiring => (explain_expiring(v)),
        }
        }))
    }
}
