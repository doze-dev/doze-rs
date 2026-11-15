//! Finding a card by name, for jump-to: a substring match, ignoring case, over card names and the
//! names of the instances a collapsed card stands for.

use crate::output::{CardId, Layout};

pub(crate) fn search(l: &Layout, q: &str) -> Vec<CardId> {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    // (starts with it, name length, card): a name that starts with the query first, then the
    // shorter, then the earlier card.
    let mut hits: Vec<(bool, usize, CardId)> = Vec::new();
    for (i, c) in l.cards.iter().enumerate() {
        let id = CardId::of(i);
        let mut best: Option<(bool, usize)> = None;
        let names = std::iter::once(&c.name).chain(l.groups.get(&c.name).into_iter().flatten());
        for n in names {
            let lower = n.to_lowercase();
            if let Some(at) = lower.find(&q) {
                let key = (at != 0, n.len());
                best = Some(best.map_or(key, |b| b.min(key)));
            }
        }
        if let Some((later, len)) = best {
            hits.push((later, len, id));
        }
    }
    hits.sort_unstable();
    hits.into_iter().map(|(_, _, id)| id).collect()
}
