//! Only trouble: the cards in trouble and the cards on a shortest path of at most three calls
//! (either way) between two of them, and the instances behind them as a stack of their own, which
//! a caller lays out like any other (few enough, that is the Go map's layout).

use std::collections::{HashMap, HashSet};

use crate::focus::MAX_HOPS;
use crate::input::{Instance, Kind};
use crate::output::{CardId, Layout};

/// The cards in trouble (`bad`, by card) and every card on a shortest path of at most
/// [`MAX_HOPS`] calls, either way, between two of them; in the layout's order.
pub(crate) fn trouble(l: &Layout, bad: &[bool]) -> Vec<CardId> {
    let n = l.cards.len();
    let is_bad = |i: usize| bad.get(i).copied().unwrap_or(false);
    let mut keep: Vec<bool> = (0..n).map(is_bad).collect();
    for s in (0..n).filter(|&i| is_bad(i) && !l.folded(CardId::of(i))) {
        let mut parent: HashMap<usize, usize> = HashMap::from([(s, s)]);
        let mut frontier = vec![s];
        for _ in 0..MAX_HOPS {
            let mut next = Vec::new();
            for &u in &frontier {
                let Some(c) = l.cards.get(u) else { continue };
                for w in c.calls.iter().chain(&c.callers).map(|w| w.ix()) {
                    if parent.contains_key(&w) || l.folded(CardId::of(w)) {
                        continue;
                    }
                    parent.insert(w, u);
                    next.push(w);
                    if is_bad(w) {
                        let mut at = w;
                        while at != s {
                            if let Some(k) = keep.get_mut(at) {
                                *k = true;
                            }
                            at = parent.get(&at).copied().unwrap_or(s);
                        }
                    }
                }
            }
            frontier = next;
        }
    }
    (0..n)
        .filter(|&i| keep.get(i) == Some(&true))
        .map(CardId::of)
        .collect()
}

/// The instances behind `names` (card names, or members of a collapsed group card), with only the
/// references among them, and the setup tasks they list that are not running (their marks): a
/// stack a caller lays out on its own, as a focus view.
#[must_use]
pub fn subset(ins: &[Instance], names: &HashSet<String>) -> Vec<Instance> {
    let mut take: HashSet<&str> = ins
        .iter()
        .filter(|v| names.contains(&v.name))
        .map(|v| v.name.as_str())
        .collect();
    let tasks: HashSet<&str> = ins
        .iter()
        .filter(|v| v.kind == Kind::Task && !v.busy)
        .map(|v| v.name.as_str())
        .collect();
    let listed: Vec<&str> = ins
        .iter()
        .filter(|v| take.contains(v.name.as_str()))
        .flat_map(|v| v.needs.iter().map(String::as_str))
        .filter(|n| tasks.contains(n))
        .collect();
    take.extend(listed);
    ins.iter()
        .filter(|v| take.contains(v.name.as_str()))
        .map(|v| {
            let mut v = v.clone();
            v.needs.retain(|n| take.contains(n.as_str()));
            v
        })
        .collect()
}

impl Layout {
    /// The cards in trouble (`bad`, one flag per card) and the cards on the shortest paths of up
    /// to three calls between them.
    #[must_use]
    pub fn trouble(&self, bad: &[bool]) -> Vec<CardId> {
        trouble(self, bad)
    }
}
