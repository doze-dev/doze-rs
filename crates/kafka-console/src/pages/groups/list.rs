//! The Groups list: `groups-list` and, when the broker lists no group, `groups-none`.

use maud::{Markup, html};

use super::common::{group_url, spans, tone};
use crate::base::Base;
use crate::format::{number, stamp};
use crate::groups_words::attention_sentence;
use crate::model::{GroupFilter, GroupPhase, GroupProtocol, GroupRow, GroupsView, RowLag};
use crate::ui::button::{Size, Variant, link, state};
use crate::ui::client;
use crate::ui::datatable::{Col, DataTable, Row, Width, data_table};
use crate::ui::components::{SegItem, seg};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::inplace;
use crate::ui::kafka::empty;
use crate::ui::layout::page_header;
use crate::ui::metrics::{Stat, deadline, slot, stat_strip};
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
        return Some(format!("offsets expire {}", stamp(at, now)));
    }
    if r.protocol == GroupProtocol::Consumer
        && let Some(last) = r.story.last.as_ref().filter(|l| !l.forming)
    {
        return Some(format!(
            "last rebalance {}",
            crate::words::live::hhmm(now - i64::try_from(last.ago_ms).unwrap_or(0))
        ));
    }
    r.last_commit_ms
        .map(|ms| format!("last commit {}", stamp(ms, now)))
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

const COLS: [Col<'static>; 6] = [
    Col::new("Group", Width::Fill),
    Col::new("Protocol", Width::Px(168)),
    Col::new("State", Width::Px(150)),
    Col::new("Members", Width::Px(88)).right(),
    Col::new("Reads", Width::Px(168)),
    Col::new("Lag", Width::Px(168)),
];

fn row(r: &GroupRow, v: &GroupsView, base: &Base) -> Row {
    let sub = name_sub(r, v.now_ms);
    Row::new(
        r.id.clone(),
        vec![
            html! {
                a class="mono name-link" href=(group_url(base, &r.id, "")) { (r.id) }
                @if let Some(s) = &sub { span class="sub ell" title=(s) { (s) } }
            },
            crate::ui::datatable::text(&protocol_word(r)),
            state_cell(r),
            html! { span class="mono" { (r.members) } },
            crate::ui::datatable::text(&r.reads.join(", ")),
            lag_cell(&r.lag),
        ],
    )
}

fn table(v: &GroupsView, base: &Base) -> Markup {
    let none = if v.filter.is_empty() {
        "No group is in this state.".to_owned()
    } else {
        format!("No group id contains {}.", v.filter)
    };
    data_table(
        &DataTable::new("groups", "Groups", &COLS)
            .roomy()
            .empty(&none),
        v.rows.iter().map(|r| row(r, v, base)).collect(),
    )
}

fn shown(v: &GroupsView) -> Markup {
    let n = number(i64::try_from(v.rows.len()).unwrap_or(0));
    let total = number(i64::try_from(v.total).unwrap_or(0));
    html! {
        @if v.truncated { "First " (slot(&n, 5)) " of " (slot(&total, 5)) " groups, by id" } @else { (slot(&n, 5)) " of " (slot(&total, 5)) " groups" }
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
                span class="nowrap" { (shown(v)) }
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
            (stat_strip(&[
                Stat::new("Groups", number(i64::try_from(v.counts[0]).unwrap_or(0))).ch(5),
                Stat::new("Stable", number(i64::try_from(v.counts[1]).unwrap_or(0))).ch(5),
                Stat::new("Rebalancing", number(i64::try_from(v.counts[2]).unwrap_or(0))).ch(5),
                Stat::new("Empty", number(i64::try_from(v.counts[3]).unwrap_or(0))).ch(5),
            ]))
            div class="attention-region" {
            @if let Some(a) = &v.attention {
                div data-extra="live-sentence" {
                    (callout(Tone::Warn, Glyph::Warn, html! {
                        (spans(&attention_sentence(a), base)) " "
                        @if a.remaining_ms > 0 { "Rejoin deadline " (deadline(i64::try_from(a.remaining_ms).unwrap_or(0))) ". " }
                        a class="lnk" href=(group_url(base, &a.id, "")) { "Open the group" }
                        @if a.last.is_some() { " or " a class="lnk" href=(group_url(base, &a.id, "/story")) { "read its story" } }
                    }))
                }
            } @else { div class="calm" { (glyph(Glyph::Check, 16, "")) "No group is rebalancing." } }
            }
        },
    )
}

/// The states as a Segmented control: each is the same list in that state (a link with no script, an in-place swap with one).
fn states(v: &GroupsView, base: &Base) -> Markup {
    let all = [
        (GroupFilter::All, "All"),
        (GroupFilter::Stable, "Stable"),
        (GroupFilter::Rebalancing, "Rebalancing"),
        (GroupFilter::Empty, "Empty"),
    ];
    let hrefs: Vec<String> = all
        .iter()
        .map(|(w, _)| {
            format!(
                "{}{}",
                base.url("/groups"),
                crate::form::encode_query(&[("q", &v.filter), ("state", w.value())])
            )
        })
        .collect();
    let items: Vec<SegItem<'_>> = all
        .iter()
        .zip(&hrefs)
        .zip(v.counts)
        .map(|(((w, label), href), n)| SegItem {
            label,
            href,
            current: v.state == *w,
            count: Some(n),
        })
        .collect();
    seg("State", &items, Some(GROUPS_VIEW))
}

/// The id of the region the toolbar and the table share: a chip or the filter changes it and nothing else of the page.
pub(super) const GROUPS_VIEW: &str = "groups-view";

fn toolbar(v: &GroupsView, base: &Base) -> Markup {
    let action = base.url("/groups");
    html! {
        (inplace::form(GROUPS_VIEW, None, "toolbar", Some("margin-top:24px"), &action, html! {
            input type="submit" value="Filter" aria-label="Filter groups" hidden data-scaffold;
            (inplace::search_box(GROUPS_VIEW, &action, "q", "Filter by group id", "Filter by group id", &v.filter))
            (states(v, base))
            span class="sp" {}
            // After the buttons: the button that was clicked is the first of its name, and typing keeps the state that is on.
            @if v.state != GroupFilter::All { input type="hidden" name="state" value=(v.state.value()); }
            span class="dim" { "Listed by id" }
        }))
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
        (inplace::region(GROUPS_VIEW, html! {
            (toolbar(v, base))
            (groups_table_fragment(v, base))
        }))
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
