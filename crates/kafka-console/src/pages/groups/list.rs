//! The Groups list: `groups-list` and, when the broker lists no group, `groups-none`.

use maud::{Markup, html};

use super::common::{group_url, spans, tone};
use crate::base::Base;
use crate::format::{ago, number, span_ms};
use crate::groups_words::attention_sentence;
use crate::model::{GroupFilter, GroupPhase, GroupProtocol, GroupRow, GroupsView, RowLag};
use crate::ui::button::{Size, Variant, link, state};
use crate::ui::client;
use crate::ui::glyph::Glyph;
use crate::ui::kafka::{empty, filter_box};
use crate::ui::layout::page_header;
use crate::ui::notice::{Tone, callout};

/// The fragments of the list: the line about a rebalancing group, and the table.
pub const GROUPS_LIVE: &str = "groups-live";
pub const GROUPS_TABLE: &str = "groups-table";

fn protocol_word(r: &GroupRow) -> String {
    match (r.protocol, &r.assignor) {
        (GroupProtocol::Consumer, _) => "consumer (KIP-848)".to_owned(),
        (_, Some(a)) if r.phase != GroupPhase::Empty => format!("classic, {a}"),
        _ => "classic".to_owned(),
    }
}

fn name_sub(r: &GroupRow, now: i64) -> Option<String> {
    if r.phase == GroupPhase::Rebalancing && r.protocol == GroupProtocol::Classic {
        return Some(format!("generation {} is forming", r.generation + 1));
    }
    if r.phase == GroupPhase::Empty
        && let Some(at) = r.expires_ms
    {
        return Some(format!("offsets expire in {}", span_ms(at - now)));
    }
    r.last_commit_ms
        .map(|ms| format!("last commit {}", ago(now - ms)))
        .or_else(|| (!r.lag.any_commit).then(|| "no commits yet".to_owned()))
}

fn state_cell(r: &GroupRow) -> Markup {
    html! {
        (state(tone(r.phase), r.phase.word()))
        @match (r.protocol, r.phase) {
            (GroupProtocol::Consumer, _) => { span class="sub" { "epoch " (r.generation) } },
            (_, GroupPhase::Stable) => { span class="sub" { "generation " (r.generation) } },
            _ => {},
        }
    }
}

fn lag_cell(l: &RowLag) -> Markup {
    if let Some((at, by)) = &l.ahead {
        return html! {
            span class="st warn" { "Past the end" }
            span class="sub" { (at) " is " (number(*by)) " beyond its log" }
        };
    }
    if !l.any_commit {
        return html! { span class="dim" { "no commits" } };
    }
    let sub = match (&l.each, l.uncommitted.first()) {
        (Some(n), _) => Some(format!("{} in each partition", number(*n))),
        (None, Some(t)) => Some(format!("{t}: no commits yet")),
        _ => None,
    };
    html! {
        @if sub.is_some() { span class="mono" { (number(l.total)) } } @else { span class="mono bright" { (number(l.total)) } }
        @if let Some(s) = sub { span class="sub" { (s) } }
    }
}

fn row(r: &GroupRow, v: &GroupsView, base: &Base) -> Markup {
    let sub = name_sub(r, v.now_ms);
    html! {
        tr {
            td class="name" {
                a class="mono" href=(group_url(base, &r.id, "")) { (r.id) }
                @if let Some(s) = sub { span class="sub" data-vary { (s) } }
            }
            td { (protocol_word(r)) }
            td { (state_cell(r)) }
            td class="r mono" { (r.members) }
            td class="mono" { (r.reads.join(", ")) }
            td { (lag_cell(&r.lag)) }
        }
    }
}

fn table(v: &GroupsView, base: &Base) -> Markup {
    html! {
        table class="t" {
            thead { tr {
                th scope="col" { "Group" } th scope="col" { "Protocol" } th scope="col" { "State" }
                th scope="col" class="r" { "Members" } th scope="col" { "Reads" } th scope="col" { "Lag" }
            } }
            tbody { @for r in &v.rows { (row(r, v, base)) } }
        }
        @if v.rows.is_empty() {
            p class="help" { "No group " @if v.filter.is_empty() { "is in this state." } @else { "id contains " span class="mono" { (v.filter) } "." } }
        }
    }
}

fn shown(v: &GroupsView) -> String {
    let n = v.rows.len();
    if v.truncated {
        format!(
            "First {n} of {} groups, by id",
            number(i64::try_from(v.total).unwrap_or(0))
        )
    } else {
        format!("{n} of {} groups", v.total)
    }
}

fn query(v: &GroupsView) -> String {
    crate::form::encode_query(&[("q", &v.filter), ("state", v.state.value())])
}

/// The table and the line under it, as a fragment.
#[must_use]
pub fn groups_table_fragment(v: &GroupsView, base: &Base) -> Markup {
    client::fragment(
        GROUPS_TABLE,
        &base.url(&format!("/fragments/{GROUPS_TABLE}{}", query(v))),
        html! {
            (table(v, base))
            div class="pager" style="justify-content:space-between" {
                a class="lnk" href=(base.url("/groups")) { "How a group gets onto this list" }
                span { (shown(v)) }
            }
        },
    )
}

/// The line about a group that is rebalancing, as a fragment (empty when none is).
#[must_use]
pub fn groups_live_fragment(v: &GroupsView, base: &Base) -> Markup {
    client::fragment(
        GROUPS_LIVE,
        &base.url(&format!("/fragments/{GROUPS_LIVE}{}", query(v))),
        html! {
            @if let Some(a) = &v.attention {
                div data-extra="live-sentence" {
                    (callout(Tone::Warn, Glyph::Warn, html! {
                        (spans(&attention_sentence(a), base)) " "
                        a class="lnk" href=(group_url(base, &a.id, "")) { "Open the group" }
                    }))
                }
            }
        },
    )
}

fn chip(v: &GroupsView, which: GroupFilter, label: &str, n: usize) -> Markup {
    let on = v.state == which;
    html! {
        button class={"fchip" @if on { " on" }} type="submit" name="state" value=(which.value()) data-ui="filter" aria-pressed=(on) {
            (label) span class="dim" { (n) }
        }
    }
}

fn toolbar(v: &GroupsView, base: &Base) -> Markup {
    html! {
        form class="toolbar" method="get" action=(base.url("/groups")) style="margin-top:24px" {
            input type="submit" value="Filter" aria-label="Filter groups" hidden data-scaffold;
            (filter_box("q", "Filter by group id", "Filter by group id", &v.filter))
            (chip(v, GroupFilter::All, "All", v.counts[0]))
            (chip(v, GroupFilter::Stable, "Stable", v.counts[1]))
            (chip(v, GroupFilter::Rebalancing, "Rebalancing", v.counts[2]))
            (chip(v, GroupFilter::Empty, "Empty", v.counts[3]))
            span class="sp" {}
            span class="dim" { "Listed by id, refreshes every 3 s while open" }
        }
    }
}

fn head(v: &GroupsView, base: &Base, sub: &str) -> Markup {
    html! {
        (page_header("Groups", html! { p class="sub" { (sub) } }, html! {}))
        div class="tabs" {
            a class="tab" href=(base.url("/topics")) { "Topics" span class="n" { (v.topics) } }
            span class="tab on" aria-current="page" { "Groups" span class="n" { (v.total) } }
        }
    }
}

fn none(v: &GroupsView, base: &Base, listen: &str) -> Markup {
    let command = format!("kcat -C -b {listen} -G payments-workers orders");
    html! {
        (head(v, base, "No consumer group has joined or committed on this broker."))
        (empty(
            Glyph::List, "", Some("teaching"),
            html! { "Groups are made by consumers, not here" },
            html! {
                "A group exists once a consumer joins with a " span class="mono" { "group.id" }
                ", or an app commits an offset for one. Start a consumer and it shows up here with its members and lag in a few seconds."
            },
            html! {
                pre class="code" style="text-align:left;width:560px;margin-bottom:24px" {
                    span class="c" { "# read orders as the group \"payments-workers\"" } "\n" (command)
                }
                (link(&base.url("/topics"), Variant::Primary, Size::Normal, None, "Go to Topics"))
            },
        ))
        p class="help" style="margin-top:20px;max-width:none" {
            "Nothing to refresh by hand. An empty group with no committed offsets is forgotten by the broker at its next sweep and leaves this list."
        }
    }
}

fn steady(v: &GroupsView, base: &Base) -> Markup {
    html! {
        (head(v, base, "Consumer groups on this broker. A group appears when a consumer joins with a group.id or commits an offset; there is nothing to create here."))
        (groups_live_fragment(v, base))
        (toolbar(v, base))
        (groups_table_fragment(v, base))
        p class="help" style="margin-top:20px" {
            "Lag is the log end minus the committed offset, worked out here from OffsetFetch and ListOffsets. The broker keeps no lag of its own. Open a group's Lag tab for the partitions."
        }
    }
}

/// The page, in the state the data calls for. `listen` is the address the commands show.
#[must_use]
pub fn groups_page(v: &GroupsView, base: &Base, listen: &str) -> Markup {
    if v.total == 0 {
        none(v, base, listen)
    } else {
        steady(v, base)
    }
}
