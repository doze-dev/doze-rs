//! `story-rebalance`: one rebalance, opened as a drawer over the Story page (which closing returns to).

use maud::{Markup, html};

use super::story_url;
use crate::base::{Base, segment};
use crate::format::clock_ms;
use crate::groups_words::span_precise;
use crate::model::{GroupHead, GroupProtocol, StoryCause, StoryGen, StoryNow, StoryView};
use crate::story::{cause_title, lasted, member_label, parts_phrase};
use crate::ui::button::{Size, State, Variant, link, state};
use crate::ui::overlay::{Kind, close, frame};

/// What the generation is now, next to its cause.
fn status(g: &StoryGen, now: &StoryNow, newest: bool) -> Markup {
    if g.forming && newest {
        let left = now.remaining_ms.zip(now.timeout_ms);
        return state(
            State::Warn,
            &left.map_or_else(
                || "Forming".to_owned(),
                |(l, of)| format!("Waiting, {} of {} left", span_precise(l), span_precise(of)),
            ),
        );
    }
    let word = match (g.settled_ms, g.formed_ms) {
        (Some(ms), _) | (None, Some(ms)) => format!("Settled in {}", span_precise(ms)),
        _ => "Began".to_owned(),
    };
    state(State::Ok, &word)
}

/// The API a cause came in on, for the link to the wire.
const fn api(g: &StoryGen) -> &'static str {
    match (g.protocol, g.cause) {
        (GroupProtocol::Consumer, _) => "ConsumerGroupHeartbeat",
        (_, StoryCause::MemberLeft) => "LeaveGroup",
        (_, StoryCause::SessionExpired) => "Heartbeat",
        _ => "JoinGroup",
    }
}

fn who_row(g: &StoryGen) -> Markup {
    html! {
        dt { "Who" }
        dd class="mono" {
            @match &g.who {
                Some(w) => { span title=(w.member) { (member_label(&w.member)) }
                    @if g.more > 0 { " and " (g.more) " more" }
                    span class="sub" {
                        @match (g.cause, &w.reason) {
                            (_, Some(r)) => { (api(g)) ", said: " q { (r) } },
                            (StoryCause::SessionExpired, _) => { "sent no heartbeat" @if let Some(ms) = w.session_ms { " for " (crate::groups_words::seconds(ms)) } },
                            _ => { (api(g)) ", sent no reason" },
                        }
                    } },
                None => { span class="dim" { "No member: " (cause_title(g.cause)) } },
            }
        }
    }
}

fn what_moves(g: &StoryGen) -> Markup {
    html! {
        dt { "What moves" }
        dd class="mono" {
            @if g.moves.is_empty() {
                span class="dim" { "not kept: the generation before is older than the ring, or this one moved nothing" }
            } @else {
                @for (member, topics) in &g.moves {
                    (parts_phrase(topics))
                    span class="sub" { "held by " (member_label(member)) ", go to whoever is in the group when it forms" }
                }
            }
        }
    }
}

/// The kv list of one rebalance.
fn facts(g: &StoryGen, head: &GroupHead, now: &StoryNow, newest: bool) -> Markup {
    let deadline = g.wait_ms.map(|w| g.at_ms + i64::try_from(w).unwrap_or(0));
    html! {
        dl class="kv sm" {
            (who_row(g))
            dt { "Began" } dd class="mono" data-vary { (clock_ms(g.at_ms)) }
            @if let (Some(at), Some(w)) = (deadline, g.wait_ms) {
                dt { "Deadline" } dd class="mono" data-vary { (clock_ms(at)) span class="sub" { "the longest rebalance timeout among members, " (span_precise(w)) } }
            }
            @if g.forming && newest {
                @if !now.waiting_for.is_empty() {
                    dt { "Not rejoined" } dd class="mono" {
                        @for m in &now.waiting_for {
                            span title=(m) { (member_label(m)) }
                            @if let Some(mem) = head.members.iter().find(|x| x.id == *m) { span class="sub" data-vary { "last heard " (mem.heard_ago_ms.map_or_else(|| "not told".to_owned(), |ms| crate::format::ago(i64::try_from(ms).unwrap_or(0)))) } }
                        }
                    }
                }
            }
            @for d in &g.dropped { dt { "Dropped" } dd class="mono" { span title=(d.who.member) { (member_label(&d.who.member)) } span class="sub" { "did not join in " (span_precise(d.waited_ms)) } } }
            dt { "Members" } dd { (g.before) " before, " (g.after) " after" }
            (what_moves(g))
            @if g.forming && newest { dt { "If nobody rejoins" } dd { "The group goes Empty. Offsets stay." } }
            @else if let Some(ms) = g.lasted_ms { dt { "Lasted" } dd class="mono" { (lasted(ms)) @if newest { span class="sub" { "so far" } } } }
        }
    }
}

/// The drawer for generation `g` of the story.
#[must_use]
pub fn rebalance_drawer(v: &StoryView, g: &StoryGen, base: &Base) -> Markup {
    let back = story_url(base, &v.head.id, "");
    let newest = v
        .generations
        .first()
        .is_some_and(|n| n.generation == g.generation && n.at_ms == g.at_ms);
    let word = if g.protocol == GroupProtocol::Classic {
        "Generation"
    } else {
        "Epoch"
    };
    frame(
        Kind::Drawer,
        "rebalance-title",
        &back,
        html! {
            h2 id="rebalance-title" { (word) " " (g.generation) }
            p style="margin:0 0 18px" { span class="cause" { (cause_title(g.cause)) } (status(g, &v.now, newest)) }
            (facts(g, &v.head, &v.now, newest))
            div class="row wrap" style="margin-top:24px" {
                (link(&base.url(&format!("/groups/{}/wire", segment(&v.head.id))), Variant::Plain, Size::Normal, None, &format!("See the {} on the wire", api(g))))
                (close("btn ghost", &back, html! { "Close" }))
            }
        },
    )
}
