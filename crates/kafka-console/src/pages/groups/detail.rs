//! The group page: `group-detail` (classic, stable or empty), `group-detail-848` (the new protocol) and `group-rebalancing`
//! (a classic group waiting for members to join again). The data chooses.

use maud::{Markup, html};

use super::common::{On, group_tabs, group_url, head, reset_and_delete, sentence_fragment, switch};
use crate::base::{Base, segment};
use crate::format::{ago, number};
use crate::groups_words::{group_sentence, span_precise};
use crate::lag::share_percent;
use crate::model::{
    GroupHead, GroupLagView, GroupMemberRow, GroupPart, GroupPhase, GroupProtocol, LagTopic,
};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::glyph_groups::{GroupGlyph, group_glyph};
use crate::ui::layout::kv;

pub(super) fn lagbar(pct: u8, n: Option<i64>) -> Markup {
    html! {
        div class="lagbar" {
            div class="tr" { i class={@if pct > 0 { "warn" }} style={"--p:" (pct) "%"} {} }
            @if let Some(n) = n { span class="n" { (number(n)) } }
        }
    }
}

/// `7 partitions`, or the topic partitions listed when few.
fn holds(m: &GroupMemberRow) -> String {
    match m.holds.len() {
        0 => "nothing yet".to_owned(),
        1 => "1 partition".to_owned(),
        n => format!("{n} partitions"),
    }
}

fn heard(m: &GroupMemberRow) -> String {
    m.heard_ago_ms.map_or_else(
        || "not told".to_owned(),
        |ms| ago(i64::try_from(ms).unwrap_or(i64::MAX)),
    )
}

/// `clickstream-0 to 5`, `orders-0, orders-2`: partitions of topics, runs folded.
pub(super) fn part_list(ps: &[GroupPart]) -> String {
    // Runs of one topic with consecutive partitions: (topic, first, last).
    let mut runs: Vec<(&str, i32, i32)> = Vec::new();
    for p in ps {
        match runs.last_mut() {
            Some((t, _, last)) if *t == p.topic && p.partition == last.saturating_add(1) => {
                *last = p.partition;
            }
            _ => runs.push((p.topic.as_str(), p.partition, p.partition)),
        }
    }
    runs.iter()
        .map(|(t, first, last)| match last - first {
            0 => format!("{t}-{first}"),
            1 => format!("{t}-{first}, {t}-{last}"),
            _ => format!("{t}-{first} to {last}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn members_table(h: &GroupHead, base: &Base) -> Markup {
    html! {
        div class="sec" style="margin:0" {
            h2 { "Members" }
            table class="t" {
                thead { tr {
                    th scope="col" { "Member" } th scope="col" { "Client id" } th scope="col" { "Host" }
                    th scope="col" { "Holds" } th scope="col" { "Last heartbeat" }
                } }
                tbody {
                    @for m in &h.members {
                        tr {
                            td class="name mono" { (m.id) @if m.leader { span class="sub" { "leader" } } }
                            td class="mono" { (m.client_id) }
                            td class="mono" { (m.host) }
                            td { (holds(m)) }
                            td class="mono" data-vary { (heard(m)) }
                        }
                    }
                }
            }
            div class="pager" style="justify-content:flex-start" {
                a class="lnk" href=(group_url(base, &h.id, "/members")) { "All columns, and who holds which partition" }
            }
        }
    }
}

fn committed_to(t: &LagTopic) -> String {
    let n = t.rows.iter().filter(|r| r.committed.is_some()).count();
    format!(
        "{n} of {} {}",
        t.partitions,
        if t.partitions == 1 {
            "partition"
        } else {
            "partitions"
        }
    )
}

fn topic_link(base: &Base, name: &str) -> Markup {
    html! { a class="mono" href=(base.url(&format!("/topics/{}", segment(name)))) { (name) } }
}

fn reads_table(topics: &[LagTopic], base: &Base) -> Markup {
    let most = topics.iter().map(|t| t.behind).max().unwrap_or(0);
    html! {
        div class="sec" style="margin:0" {
            h2 { "Reads" }
            table class="t" {
                thead { tr {
                    th scope="col" { "Topic" } th scope="col" class="r" { "Partitions" }
                    th scope="col" { "Committed to" } th scope="col" { "Lag" }
                } }
                tbody {
                    @for t in topics {
                        tr {
                            td class="name" { (topic_link(base, &t.name)) }
                            td class="r mono" { (t.partitions) }
                            td { (committed_to(t)) }
                            td { (lagbar(share_percent(t.behind, most), Some(t.behind))) }
                        }
                    }
                }
            }
        }
    }
}

fn the_group(h: &GroupHead, v: &GroupLagView) -> Markup {
    let last = h.last_commit_ms.map(|ms| ago(h.now_ms - ms));
    let session = h
        .members
        .iter()
        .map(|m| m.session_ms)
        .max()
        .filter(|s| *s > 0);
    let rebalance = h.members.iter().filter_map(|m| m.rebalance_ms).max();
    let secs = |ms: u64| html! { span class="mono" { (crate::groups_words::seconds(ms)) } };
    let mut rows: Vec<(&str, Markup)> = vec![
        (
            "Protocol type",
            html! { span class="mono" { (h.protocol_type.as_deref().unwrap_or("consumer")) } },
        ),
        (
            "Assignor",
            html! { span class="mono" { (h.assignor.as_deref().unwrap_or("none yet")) } },
        ),
        ("Generation", html! { span class="mono" { (h.generation) } }),
        (
            "Leader",
            html! { span class="mono" { (h.leader.as_deref().unwrap_or("none yet")) } },
        ),
    ];
    if let Some(s) = session {
        rows.push(("Session timeout", secs(s)));
    }
    if let Some(r) = rebalance {
        rows.push(("Rebalance timeout", secs(r)));
    }
    if let Some(l) = last {
        rows.push(("Last commit", html! { span class="mono" data-vary { (l) } }));
    }
    let _ = v;
    html! {
        div class="card" {
            h2 { "The group" }
            (kv(rows))
            p class="help" style="margin-top:12px" {
                "Timeouts are what the member asked for when it joined. The leader computes the assignment; the broker only hands it out."
            }
        }
    }
}

fn classic(v: &GroupLagView, base: &Base) -> Markup {
    let h = &v.head;
    html! {
        (head(h, sentence_fragment(h, base, &group_sentence(h, &v.topics), true), reset_and_delete(h, base)))
        (switch(h, base, ""))
        (group_tabs(h, base, On::Members))
        div class="split" {
            div class="stack-y" {
                @if !h.members.is_empty() { (members_table(h, base)) }
                (reads_table(&v.topics, base))
            }
            (the_group(h, v))
        }
    }
}

fn status(m: &GroupMemberRow, target_epoch: i32) -> (&'static str, &'static str) {
    if m.holds == m.target && m.epoch == target_epoch {
        ("ok", "In step")
    } else {
        ("warn", "Reconciling")
    }
}

fn consumer_member(m: &GroupMemberRow, target_epoch: i32) -> Markup {
    let (class, word) = status(m, target_epoch);
    html! {
        tr {
            td class="name mono" { (m.id) }
            td class="mono" { (m.client_id) }
            td class="r mono" { (m.epoch) }
            td class="mono" { (part_list(&m.target)) }
            td class="mono" { (part_list(&m.holds)) }
            td { span class={"st " (class)} { (word) } }
        }
    }
}

fn where_a_group_can_be(h: &GroupHead) -> Markup {
    let row = |word: &str, sub: &str, here: bool, phase: GroupPhase| {
        let on = here && h.phase == phase;
        html! {
            div class={"r" @if on { " ok" }} {
                span class={"gl" @if !on { " dim" }} { (if on { glyph(Glyph::Check, 16, "") } else { group_glyph(GroupGlyph::Dot, 16, "") }) }
                div {
                    @if on { b class="bright" { (word) } } @else { (word) }
                    span class="sub" { (sub) @if on { " This group, now." } }
                }
                span class="v" { @if on { "now" } }
            }
        }
    };
    html! {
        div class="card" {
            h2 { "Where a group can be" }
            div class="rules" {
                (row("Empty", "No member. Offsets are kept until retention.", true, GroupPhase::Empty))
                (row("Assigning", "The epoch moved; targets are computed at the next heartbeat, at least 1 s after the last time.", true, GroupPhase::Assigning))
                (row("Reconciling", "Targets differ from what members hold. A member hands a partition back before another takes it.", true, GroupPhase::Reconciling))
                (row("Stable", "Every member holds its target.", true, GroupPhase::Stable))
            }
        }
    }
}

fn consumer_reads(topics: &[LagTopic], base: &Base) -> Markup {
    html! {
        div class="card" {
            h2 { "Reads" }
            table class="t flat" {
                thead { tr { th scope="col" { "Topic" } th scope="col" { "Partitions" } th scope="col" { "Lag" } } }
                tbody {
                    @for t in topics {
                        @let ps: Vec<String> = t.rows.iter().filter(|r| t.holders.is_empty() || r.committed.is_some()).map(|r| r.partition.to_string()).collect();
                        @let behind: Vec<i64> = t.rows.iter().map(|r| r.lag.behind()).collect();
                        tr {
                            td class="name" { (topic_link(base, &t.name)) }
                            td class="mono" { (ps.join(", ")) }
                            td {
                                @if let Some(first) = behind.first().filter(|f| behind.len() > 1 && behind.iter().all(|n| n == *f)) { (number(*first)) " in each" }
                                @else { (number(t.behind)) }
                            }
                        }
                    }
                }
            }
            p class="help" style="margin-top:12px" {
                "Offsets are committed with the member epoch. A commit at an old epoch is refused with STALE_MEMBER_EPOCH."
            }
        }
    }
}

fn consumer(v: &GroupLagView, base: &Base) -> Markup {
    let h = &v.head;
    let target = h.target_epoch.unwrap_or(h.generation);
    let big = |label: &str, value: Markup, help: Markup| {
        html! { div class="card" { div class="eyebrow" { (label) } (value) p class="help" { (help) } } }
    };
    html! {
        (head(h, sentence_fragment(h, base, &group_sentence(h, &v.topics), true), reset_and_delete(h, base)))
        (switch(h, base, ""))
        (group_tabs(h, base, On::Members))
        div class="sec" { div class="g4" {
            (big("Group epoch", html! { div class="big" { (h.generation) } }, html! { "Moves whenever membership, subscriptions or topics change." }))
            (big("Targets computed for", html! { div class="big" { (target) } }, html! { @if target == h.generation { "The same epoch, so every member has its target." } @else { "An older epoch: the targets are computed at the next heartbeat." } }))
            (big("Assignor", html! { div class="big" style="font-size:28px;line-height:48px" { (h.assignor.as_deref().unwrap_or("uniform")) } }, html! { "The broker assigns. Clients do not elect a leader." }))
            (big("Session timeout", html! { div class="big" { @match h.session_ms { Some(ms) => { (ms / 1000) small { "s" } }, None => { "—" } } } }, html! { "From " span class="mono" { "--consumer-session-timeout" } "." }))
        } }
        div class="sec" {
            h2 { "Members, target and assigned" }
            p class="lede" { "Target is what the broker wants this member to hold at the group epoch. Assigned is what it holds now. They are equal here; when they differ the group is Reconciling." }
            table class="t" data-extra="teaching" {
                thead { tr {
                    th scope="col" { "Member" } th scope="col" { "Client id" } th scope="col" class="r" { "Member epoch" }
                    th scope="col" { "Target" } th scope="col" { "Assigned" } th scope="col" { "Status" }
                } }
                tbody { @for m in &h.members { (consumer_member(m, target)) } }
            }
        }
        div class="g2" { (where_a_group_can_be(h)) (consumer_reads(&v.topics, base)) }
    }
}

fn waiting_card(h: &GroupHead) -> Markup {
    let r = h.rebalance.as_ref();
    let (remaining, timeout) = r.map_or((0, 0), |r| (r.remaining_ms, r.timeout_ms));
    let pct = remaining
        .saturating_mul(100)
        .checked_div(timeout)
        .map_or(100, |left| 100 - u8::try_from(left.min(100)).unwrap_or(100));
    html! {
        div class="card" {
            div class="row sb" {
                div {
                    div class="eyebrow" { "Waiting for" }
                    div class="mono bright" style="font-size:22px;line-height:30px;margin-top:4px" {
                        @match r.and_then(|r| r.waiting_for.first()) { Some(w) => (w), None => "the leader's assignment" }
                    }
                }
                div style="text-align:right" { div class="big" data-vary { (span_precise(remaining).trim_end_matches(" s")) small { "s of " (span_precise(timeout)) " left" } } }
            }
            div class="lagbar" style="margin:20px 0 8px" { div class="tr" { i class="warn" style={"--p:" (pct) "%"} {} } }
            div class="row sb" {
                span class="mono dim" data-vary { (span_precise(r.map_or(0, |r| r.began_ago_ms))) " since it began" }
                span class="mono dim" data-vary { (span_precise(remaining)) " to the deadline" }
            }
            p class="help" style="margin-top:16px;max-width:none" {
                "Members that have not joined again by the deadline are dropped."
            }
        }
    }
}

fn what_next(h: &GroupHead) -> Markup {
    let waiting = h
        .rebalance
        .as_ref()
        .and_then(|r| r.waiting_for.first())
        .cloned()
        .unwrap_or_else(|| "the members".to_owned());
    html! {
        div class="card" {
            h2 { "What happens next" }
            div class="rules" {
                div class="r ok" {
                    span class="gl" { (glyph(Glyph::Check, 16, "")) }
                    div { (waiting) " rejoins before the deadline" span class="sub" { "Generation " (h.generation + 1) " forms with the members that joined, and the leader assigns the partitions." } }
                    span class="v" {}
                }
                div class="r no" {
                    span class="gl" { (glyph(Glyph::Warn, 16, "")) }
                    div { "It does not" span class="sub" { (waiting) " is dropped and, if it was the last, the group goes Empty. Its committed offsets stay." } }
                    span class="v" {}
                }
            }
        }
    }
}

fn rebalancing_members(h: &GroupHead) -> Markup {
    html! {
        div class="sec" style="margin:40px 0 0" {
            h2 { "Members" }
            table class="t" {
                thead { tr {
                    th scope="col" { "Member" } th scope="col" { "State" }
                    th scope="col" { "Held at generation " (h.generation) } th scope="col" { "Last heard" }
                } }
                tbody {
                    @for m in &h.members {
                        tr {
                            td class="name mono" { (m.id) @if m.leader { span class="sub" { "leader at generation " (h.generation) } } }
                            td { @if m.rejoined { span class="st ok" { "Rejoined" } } @else { span class="st warn" { "Not rejoined" } } }
                            td class="mono" { (part_list(&m.holds)) }
                            td class="mono" data-vary { (heard(m)) @if !m.rejoined { span class="sub" { "told to rejoin" } } }
                        }
                    }
                }
            }
        }
    }
}

fn rebalancing(v: &GroupLagView, base: &Base) -> Markup {
    let h = &v.head;
    html! {
        (head(h, sentence_fragment(h, base, &group_sentence(h, &v.topics), true), reset_and_delete(h, base)))
        (switch(h, base, ""))
        (group_tabs(h, base, On::Members))
        div class="g2" style="align-items:stretch" { (waiting_card(h)) (what_next(h)) }
        (rebalancing_members(h))
    }
}

/// The group's page in the state its data calls for.
#[must_use]
pub fn group_page(v: &GroupLagView, base: &Base) -> Markup {
    let h = &v.head;
    match (h.protocol, h.phase) {
        (GroupProtocol::Consumer, _) => consumer(v, base),
        (GroupProtocol::Classic, GroupPhase::Rebalancing) => rebalancing(v, base),
        _ => classic(v, base),
    }
}
