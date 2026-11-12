//! The members page (`group-members`) and the member's drawer (`member-detail`): a drawer over the members page, which a
//! direct visit to `/groups/{group}/members/{member}` shows with the drawer open on it.

use maud::{Markup, html};

use super::common::{On, group_tabs, group_url, head, reset_and_delete, sentence_fragment, switch};
use super::detail::part_list;
use crate::base::{Base, segment};
use crate::format::ago;
use crate::groups_words::{group_sentence, number_word};
use crate::model::{GroupHead, GroupLagView, GroupMemberRow, GroupPart, GroupProtocol, LagTopic};
use crate::ui::button::{Size, Variant, link};
use crate::ui::overlay::{Kind, close, frame};
use crate::ui::table::middle;

fn role(h: &GroupHead, m: &GroupMemberRow) -> &'static str {
    match (h.protocol, m.leader) {
        (GroupProtocol::Consumer, _) => "Member",
        (_, true) => "Leader",
        (_, false) => "Follower",
    }
}

fn secs(ms: u64) -> String {
    crate::groups_words::seconds(ms)
}

fn heard(m: &GroupMemberRow) -> String {
    m.heard_ago_ms.map_or_else(
        || "not told".to_owned(),
        |ms| ago(i64::try_from(ms).unwrap_or(0)),
    )
}

fn row(h: &GroupHead, m: &GroupMemberRow, base: &Base, open: Option<&str>) -> Markup {
    html! {
        tr class=[(open == Some(m.id.as_str())).then_some("sel")] {
            td class="name mid-cell" { a class="mono" href=(group_url(base, &h.id, &format!("/members/{}", segment(&m.id)))) data-dialog { (middle(&m.id)) } }
            td class="mono" { (m.client_id) }
            td class="mono" { (m.host) }
            td { (role(h, m)) }
            td class="r mono" { (m.epoch) }
            td class="mono" { @if m.session_ms > 0 { (secs(m.session_ms)) } @else { "—" } }
            td class="mono" data-vary { (heard(m)) }
        }
    }
}

/// Who holds the partitions of one topic, as tags.
fn topic_row(t: &LagTopic, members: &[GroupMemberRow], base: &Base) -> Markup {
    let holder_of = |p: i32| -> Option<&str> {
        members
            .iter()
            .find(|m| {
                m.holds.contains(&GroupPart {
                    topic: t.name.clone(),
                    partition: p,
                })
            })
            .map(|m| m.id.as_str())
    };
    let parts: Vec<i32> = (0..i32::try_from(t.partitions).unwrap_or(0)).collect();
    let owners: Vec<&str> = {
        let mut o: Vec<&str> = parts.iter().filter_map(|p| holder_of(*p)).collect();
        o.dedup();
        o
    };
    let all_one = owners.len() == 1 && parts.iter().all(|p| holder_of(*p).is_some());
    html! {
        div class="row wrap" style="gap:10px" {
            a class="lnk mono" href=(base.url(&format!("/topics/{}", segment(&t.name)))) style="width:160px" { (t.name) }
            @for p in &parts { span class="tag" { (p) } }
            @if all_one {
                span class="dim" { "all " (number_word(parts.len())) ": " (owners[0]) }
            } @else {
                span class="dim" {
                    @for (i, p) in parts.iter().enumerate() {
                        @if i > 0 { ", " }
                        (p) ": " @match holder_of(*p) { Some(m) => (m), None => "nobody" }
                    }
                }
            }
        }
    }
}

fn who_holds(v: &GroupLagView, base: &Base) -> Markup {
    let h = &v.head;
    let read: Vec<&LagTopic> = v
        .topics
        .iter()
        .filter(|t| {
            t.exists
                && h.members
                    .iter()
                    .any(|m| m.holds.iter().any(|p| p.topic == t.name))
        })
        .collect();
    let total: usize = h.members.iter().map(|m| m.holds.len()).sum();
    let at = match h.protocol {
        GroupProtocol::Classic => format!("the leader computed at generation {}", h.generation),
        GroupProtocol::Consumer => format!("the broker targeted at epoch {}", h.generation),
    };
    html! {
        div class="sec" style="margin-top:40px" {
            h2 { "Who holds which partition" }
            p class="lede" { "The assignment " (at) ". Each box is a partition of a topic this group reads." }
            div class="card" data-extra="teaching" {
                div class="stack-y" { @for t in &read { (topic_row(t, &h.members, base)) } }
                div class="hr" {}
                p class="help" style="max-width:none" {
                    @if h.members.len() == 1 {
                        "This is one process reading " (number_word(total)) " partitions. Lag grows when a single consumer cannot keep up; a second instance with the same "
                        span class="mono" { "group.id" } " would split these partitions at the next rebalance."
                    } @else {
                        (h.members.len()) " members share these partitions. A member that leaves hands its partitions to the others at the next rebalance."
                    }
                }
            }
        }
    }
}

/// `group-members`.
#[must_use]
pub fn members_page(v: &GroupLagView, base: &Base) -> Markup {
    members_page_open(v, base, None)
}

/// The Members page behind a member's drawer: the member the drawer is about is the selected row, as the board draws it.
#[must_use]
pub fn members_page_open(v: &GroupLagView, base: &Base, open: Option<&str>) -> Markup {
    let h = &v.head;
    html! {
        (head(h, sentence_fragment(h, base, &group_sentence(h, &v.topics), false), reset_and_delete(h, base)))
        (switch(h, base, "/members"))
        (group_tabs(h, base, On::Members))
        table class="t" {
            thead { tr {
                th scope="col" { "Member id" } th scope="col" { "Client id" } th scope="col" { "Host" }
                th scope="col" { "Role" } th scope="col" class="r" { "Generation" } th scope="col" { "Session" }
                th scope="col" { "Last heartbeat" }
            } }
            tbody { @for m in &h.members { (row(h, m, base, open)) } }
        }
        (who_holds(v, base))
    }
}

/// `member-detail`: one member, as a drawer over the members page (which closing returns to).
#[must_use]
pub fn member_drawer(h: &GroupHead, m: &GroupMemberRow, base: &Base) -> Markup {
    let topics: Vec<&str> = {
        let mut t: Vec<&str> = m.holds.iter().map(|p| p.topic.as_str()).collect();
        t.dedup();
        t
    };
    let first = topics.first().copied();
    let back = group_url(base, &h.id, "/members");
    frame(
        Kind::Drawer,
        "member-title",
        &back,
        html! {
            h2 class="mono" id="member-title" { (m.id) }
            p class="dim" style="margin:0 0 20px" {
                "A member of " a class="lnk mono" href=(group_url(base, &h.id, "")) { (h.id) }
                @if h.protocol == GroupProtocol::Classic { " at generation " (m.epoch) } @else { " at member epoch " (m.epoch) }
                ". " (h.phase.word()) "."
            }
            dl class="kv sm" {
                dt { "Client id" } dd class="mono" { (m.client_id) }
                dt { "Host" } dd class="mono" { (m.host) }
                dt { "Role" } dd {
                    @match (h.protocol, m.leader) {
                        (GroupProtocol::Consumer, _) => "The broker assigns; a member does not lead",
                        (_, true) => { "Leader: computed the " (h.assignor.as_deref().unwrap_or("")) " assignment" },
                        (_, false) => "Follower: takes the assignment the leader computed",
                    }
                }
                dt { "Generation" } dd class="mono" { (m.epoch) }
                dt { "Session timeout" } dd class="mono" {
                    @if m.session_ms > 0 { (secs(m.session_ms)) } @else { "—" }
                    span class="sub" data-vary { (heard(m)) " since its last heartbeat" }
                }
                dt { "Rebalance timeout" } dd class="mono" { @match m.rebalance_ms { Some(r) => (secs(r)), None => "not sent" } }
                dt { "Static instance id" } dd {
                    @match &m.instance { Some(i) => span class="mono" { (i) }, None => span class="dim" { "none: a restart makes a new member" } }
                }
                dt { "Holds" } dd class="mono" {
                    @for (i, part) in part_list(&m.holds).split(", ").enumerate() { @if i > 0 { br; } (part) }
                    @if m.holds.is_empty() { "nothing" }
                }
            }
            div class="row wrap" style="margin-top:24px" {
                (link(&base.url("/live"), Variant::Plain, Size::Normal, None, "Open the wire in Live"))
                @if let Some(t) = first { (link(&base.url(&format!("/topics/{}", segment(t))), Variant::Plain, Size::Normal, None, &format!("Open {t}"))) }
                (close("btn ghost", &back, html! { "Close" }))
            }
        },
    )
}
