//! The sentences that say why a group rebalanced, when the broker's ring of notes has it (slice 9f): what a rebalancing
//! group opens with, what it waits for, and the last rebalance of a group that is in step.

use super::{GroupAttention, GroupHead, S, list_and, span_precise};

/// How the group came to be rebalancing, when the group has said: `fulfilment-py-2 left 3 s ago, so the group is rebalancing.`
pub(super) fn lead_in(h: &GroupHead) -> Option<Vec<S>> {
    let last = h.story.last.as_ref().filter(|l| l.forming)?;
    let (who, clause) = crate::story::last_clause(last);
    let mut out = Vec::new();
    if let Some(w) = who {
        out.push(S::strong(w));
    }
    out.push(S::text(format!("{clause}, so the group is rebalancing.")));
    Some(out)
}

/// " It is waiting for py-1 to rejoin: 2.1 s of the 5 s rebalance timeout are left."
pub(super) fn waiting_clause(h: &GroupHead) -> Vec<S> {
    match &h.rebalance {
        Some(r) if !r.waiting_for.is_empty() => vec![
            S::text(" It is waiting for "),
            S::strong(list_and(&r.waiting_for)),
            S::text(format!(
                " to rejoin: {} of the {} rebalance timeout are left.",
                span_precise(r.remaining_ms),
                span_precise(r.timeout_ms),
            )),
        ],
        _ => vec![S::text(
            " Every member has joined again and the leader is working out who holds what.",
        )],
    }
}

/// " The last rebalance was 6 h ago: payments-3c joined." (what the group has said of itself).
pub(super) fn last_rebalance(h: &GroupHead) -> Vec<S> {
    let Some(last) = h.story.last.as_ref().filter(|l| !l.forming) else {
        return Vec::new();
    };
    let ago = crate::story::ago_near(last.ago_ms);
    let mut out = vec![S::text(format!(" The last rebalance was {ago}: "))];
    let verb = match last.cause {
        crate::model::StoryCause::MemberLeft => "left",
        crate::model::StoryCause::SessionExpired => "timed out",
        crate::model::StoryCause::MemberJoined => "joined",
        _ => "",
    };
    match (&last.who, verb) {
        (Some(w), v) if !v.is_empty() => {
            out.push(S::strong(crate::story::member_label(w)));
            out.push(S::text(format!(" {v}.")));
        }
        _ => out.push(S::text(format!(
            "{}.",
            crate::story::cause_phrase(last.cause)
        ))),
    }
    out
}

/// The line above the list while a classic group is rebalancing.
#[must_use]
pub fn attention_sentence(a: &GroupAttention) -> Vec<S> {
    let mut v = vec![S::strong(format!("{} is rebalancing.", a.id)), S::text(" ")];
    if let Some(last) = a.last.as_ref().filter(|l| l.forming) {
        let (who, clause) = crate::story::last_clause(last);
        if let Some(w) = who {
            v.push(S::mono(w));
        }
        v.push(S::text(format!("{clause} and the group is waiting for ")));
        v.push(S::mono(if a.waiting_for.is_empty() {
            "the leader".to_owned()
        } else {
            list_and(&a.waiting_for)
        }));
        v.push(S::text(" to rejoin."));
    } else if a.waiting_for.is_empty() {
        v.push(S::text(
            "Every member has joined again and the leader is working out who holds what.",
        ));
    } else {
        v.push(S::text("It is waiting for "));
        v.push(S::mono(list_and(&a.waiting_for)));
        v.push(S::text(" to rejoin."));
    }
    v
}
