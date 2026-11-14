//! The sentences that say why a group rebalanced, when the broker's ring of notes has it (slice 9f): what a rebalancing
//! group opens with, what it waits for, and the last rebalance of a group that is in step.

use super::{GroupAttention, S, list_and};

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
