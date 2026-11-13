//! Focus views, made from a laid-out map: a card's neighbourhood, and the cards in trouble with what
//! joins them.
//!
//! A neighbourhood is the card and what it reaches in one to three calls either way, in columns by
//! path: left of the card the chains of its callers (calls it, two steps up, three), right of it the
//! chains of what it calls, then two columns for paths that turn (out through its callers and back
//! down: its siblings and theirs; out through what it calls and back up). A card's column is set by
//! its shortest path, so a column means the same at every hop count and a card never moves when the
//! hops grow. Every call between shown cards is drawn, as a plain curve.
//!
//! Only trouble is in [`crate::trouble`].

use std::collections::{HashMap, HashSet};

use crate::draw::link;
use crate::geom::{Pt, Rect};
use crate::index::Index;
use crate::output::{Caption, CaptionKind, Card, CardId, Edge, Layout, Mode};

/// The furthest a neighbourhood or a trouble path reaches.
pub const MAX_HOPS: u8 = 3;
/// Cards down a column before it wraps into another beside it.
const PER: usize = 8;
const TOP: f64 = 16.0;
const CAP: f64 = 40.0;
const ROW: f64 = 16.0;
const SUB_GAP: f64 = 28.0;
const COL_GAP: f64 = 120.0;

/// The columns, left to right.
const COLUMNS: [&str; 9] = [
    "3 steps up",
    "2 steps up",
    "calls it",
    "the card",
    "it calls",
    "2 steps down",
    "3 steps down",
    "through its callers",
    "through what it calls",
];
const CENTRE: usize = 3;
const VIA_CALLERS: usize = 7;
const VIA_CALLS: usize = 8;

/// The column a card reached from a card in `from` by a step up (to a caller) or down lands in.
fn step(from: usize, up: bool) -> usize {
    match (from, up) {
        (CENTRE, true) => CENTRE - 1,
        (CENTRE, false) => CENTRE + 1,
        (1..=2, true) => from - 1,
        (0..=2, false) => VIA_CALLERS,
        (4..=5, false) => from + 1,
        (4..=6, true) => VIA_CALLS,
        (c, _) => c,
    }
}

/// Pure paths before turning ones.
fn rank(col: usize) -> usize {
    match col {
        VIA_CALLERS => 1,
        VIA_CALLS => 2,
        _ => 0,
    }
}

/// Each card the neighbourhood reaches, with its column, in the order reached.
pub(crate) fn reach(l: &Layout, centre: CardId, hops: u8) -> Vec<(CardId, usize)> {
    let mut col: HashMap<CardId, usize> = HashMap::from([(centre, CENTRE)]);
    let mut out = vec![(centre, CENTRE)];
    let mut frontier = vec![centre];
    for _ in 0..hops.min(MAX_HOPS) {
        let mut found: Vec<(CardId, usize)> = Vec::new();
        for &u in &frontier {
            let (Some(card), Some(&cu)) = (l.card(u), col.get(&u)) else {
                continue;
            };
            let steps = card.callers.iter().map(|&w| (w, true));
            for (w, up) in steps.chain(card.calls.iter().map(|&w| (w, false))) {
                if col.contains_key(&w) || l.folded(w) {
                    continue;
                }
                let c = step(cu, up);
                match found.iter_mut().find(|(x, _)| *x == w) {
                    Some(f) if rank(c) < rank(f.1) => f.1 = c,
                    Some(_) => {}
                    None => found.push((w, c)),
                }
            }
        }
        for &(w, c) in &found {
            col.insert(w, c);
        }
        frontier = found.iter().map(|f| f.0).collect();
        out.extend(found);
    }
    out
}

/// A card's neighbourhood as a layout of its own (see the module docs); `None` for a card the map
/// does not have or that is folded.
pub(crate) fn hood(l: &Layout, centre: CardId, hops: u8) -> Option<Layout> {
    if l.card(centre).is_none() || l.folded(centre) {
        return None;
    }
    let reached = reach(l, centre, hops);
    let mut cols: Vec<Vec<CardId>> = vec![Vec::new(); COLUMNS.len()];
    for &(c, k) in &reached {
        if let Some(v) = cols.get_mut(k) {
            v.push(c);
        }
    }
    let (rects, caps, w, h) = place(l, &cols);
    let shown: Vec<CardId> = cols.iter().flatten().copied().collect();
    Some(build(l, &shown, &rects, caps, (w, h)))
}

/// Each shown card's box, the columns' captions, and the drawing's size.
fn place(l: &Layout, cols: &[Vec<CardId>]) -> (HashMap<CardId, Rect>, Vec<Caption>, f64, f64) {
    let size = |c: &CardId| l.card(*c).map_or((214.0, 74.0), |c| (c.rect.w, c.rect.h));
    let height = |v: &[CardId]| v.iter().take(PER).map(|c| size(c).1 + ROW).sum::<f64>() - ROW;
    let tallest = cols.iter().map(|v| height(v)).fold(0.0, f64::max);
    let top = TOP + CAP;
    let mut rects = HashMap::new();
    let mut caps = Vec::new();
    let mut x = 24.0;
    for (k, v) in cols.iter().enumerate().filter(|(_, v)| !v.is_empty()) {
        caps.push(Caption {
            kind: CaptionKind::Column,
            text: COLUMNS.get(k).copied().unwrap_or("").to_string(),
            sub: if k == CENTRE {
                String::new()
            } else {
                v.len().to_string()
            },
            at: Pt::new(x, TOP),
        });
        for chunk in v.chunks(PER) {
            let w = chunk.iter().map(|c| size(c).0).fold(0.0, f64::max);
            let mut y = top + (tallest - height(chunk)) / 2.0;
            for c in chunk {
                let (cw, ch) = size(c);
                rects.insert(*c, Rect::new(x, y, cw, ch));
                y += ch + ROW;
            }
            x += w + SUB_GAP;
        }
        x += COL_GAP - SUB_GAP;
    }
    let w = (x - COL_GAP + 24.0).max(600.0);
    (rects, caps, w, top + tallest + 24.0)
}

/// A layout of the `shown` cards at `rects`, every call between them drawn.
fn build(
    l: &Layout,
    shown: &[CardId],
    rects: &HashMap<CardId, Rect>,
    captions: Vec<Caption>,
    (w, h): (f64, f64),
) -> Layout {
    let new: HashMap<CardId, CardId> = shown
        .iter()
        .enumerate()
        .map(|(i, &c)| (c, CardId::of(i)))
        .collect();
    let map = |v: &[CardId]| v.iter().filter_map(|c| new.get(c).copied()).collect();
    let cards: Vec<Card> = shown
        .iter()
        .filter_map(|&c| {
            let base = l.card(c)?;
            Some(Card {
                rect: rects.get(&c).copied().unwrap_or_default(),
                calls: map(&base.calls),
                callers: map(&base.callers),
                uses: Vec::new(),
                ..base.clone()
            })
        })
        .collect();
    let counts: HashMap<(CardId, CardId), u32> =
        l.edges.iter().map(|e| ((e.from, e.to), e.count)).collect();
    let mut edges = Vec::new();
    for &a in shown {
        let (Some(card), Some(ra)) = (l.card(a), rects.get(&a)) else {
            continue;
        };
        for b in &card.calls {
            if let (Some(rb), Some(&na), Some(&nb)) = (rects.get(b), new.get(&a), new.get(b)) {
                edges.push(Edge {
                    from: na,
                    to: nb,
                    path: link(ra, rb),
                    count: counts.get(&(a, *b)).copied().unwrap_or(1),
                    back: rb.x < ra.x,
                    across: false,
                });
            }
        }
    }
    let by_name = cards
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.clone(), CardId::of(i)))
        .collect();
    let index = Index::build(&cards, &[], &edges);
    Layout {
        mode: Mode::Focus,
        bounds: Rect::new(0.0, 0.0, w, h),
        band: None,
        captions,
        region_of: vec![None; cards.len()],
        cards,
        edges,
        detail: Pt::new(w - 12.0 - 448.0, 14.0),
        groups: l.groups.clone(),
        regions: Vec::new(),
        tile: None,
        folded: Vec::new(),
        lines: Vec::new(),
        gutters: None,
        index,
        by_name,
    }
}

impl Layout {
    /// A card's neighbourhood to `hops` calls (at most [`MAX_HOPS`]) as a layout of its own, in
    /// columns by path (see the module docs). `None` for a folded card or one the map lacks.
    #[must_use]
    pub fn neighbourhood(&self, centre: CardId, hops: u8) -> Option<Layout> {
        hood(self, centre, hops)
    }

    /// The instance names behind `ids`: each card's name and, for a collapsed group's card, its
    /// members'.
    #[must_use]
    pub fn names(&self, ids: &[CardId]) -> HashSet<String> {
        let mut out = HashSet::new();
        for c in ids.iter().filter_map(|&c| self.card(c)) {
            out.insert(c.name.clone());
            out.extend(self.groups.get(&c.name).into_iter().flatten().cloned());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{COLUMNS, reach};
    use crate::input::{Instance, Kind};
    use crate::output::Layout;
    use crate::trouble::subset;
    use crate::{Options, layout};

    fn stack(calls: &[(&str, &[&str])]) -> Layout {
        let ins: Vec<Instance> = calls
            .iter()
            .map(|(n, needs)| Instance::new(n, Kind::App, needs))
            .collect();
        layout(&ins, &Options::default())
    }

    fn columns(l: &Layout, centre: &str, hops: u8) -> Vec<(String, &'static str)> {
        let c = l.card_named(centre).unwrap();
        let mut v: Vec<(String, &'static str)> = reach(l, c, hops)
            .into_iter()
            .map(|(id, k)| (l.card(id).unwrap().name.clone(), COLUMNS[k]))
            .collect();
        v.sort();
        v
    }

    /// Columns by path, and a column keeps its meaning (and its cards) as the hops grow.
    #[test]
    fn a_neighbourhood_is_columns_by_path() {
        let l = stack(&[
            ("up2", &["up1"]),
            ("up1", &["c", "sib"]),
            ("c", &["d1"]),
            ("d1", &["d2"]),
            ("d2", &["d3"]),
            ("d3", &[]),
            ("sib", &[]),
            ("co", &["d1"]),
            ("far", &["up2"]),
        ]);
        let one = columns(&l, "c", 1);
        assert_eq!(
            one,
            [
                ("c".into(), "the card"),
                ("d1".into(), "it calls"),
                ("up1".into(), "calls it")
            ]
        );
        let two = columns(&l, "c", 2);
        for x in &one {
            assert!(two.contains(x), "{x:?} kept its column");
        }
        for want in [
            ("up2", "2 steps up"),
            ("d2", "2 steps down"),
            ("sib", "through its callers"),
            ("co", "through what it calls"),
        ] {
            assert!(
                two.contains(&(want.0.into(), want.1)),
                "{want:?} in {two:?}"
            );
        }
        assert_eq!(two.len(), 7);
        let three = columns(&l, "c", 3);
        assert!(three.contains(&("far".into(), "3 steps up")));
        assert!(three.contains(&("d3".into(), "3 steps down")));
        // The focus layout: every shown card once, no two on each other, the calls among them.
        let h = l.neighbourhood(l.card_named("c").unwrap(), 2).unwrap();
        assert_eq!(h.cards.len(), 7);
        for (i, a) in h.cards.iter().enumerate() {
            for b in h.cards.iter().skip(i + 1) {
                assert!(!a.rect.intersects(&b.rect), "{} on {}", a.name, b.name);
            }
        }
        assert!(h.edges.iter().all(|e| h.card(e.from).is_some()));
        assert_eq!(h.edges.len(), 6, "up2>up1 up1>c up1>sib c>d1 d1>d2 co>d1");
        assert_eq!(
            h.card_named("c")
                .and_then(|c| h.hit(h.card(c)?.rect.center())),
            h.card_named("c")
        );
    }

    /// Trouble and the shortest paths of up to three calls between two in trouble; nothing else.
    #[test]
    fn trouble_and_what_joins_it() {
        let l = stack(&[
            ("a", &["b"]),
            ("b", &["c"]),
            ("c", &["d"]),
            ("d", &["e"]),
            ("e", &["f"]),
            ("f", &["g"]),
            ("g", &["h"]),
            ("h", &[]),
            ("x", &["c"]),
            ("lone", &[]),
        ]);
        let bad_names = ["a", "c", "h", "lone"];
        let bad: Vec<bool> = l
            .cards
            .iter()
            .map(|c| bad_names.contains(&c.name.as_str()))
            .collect();
        let got: HashSet<String> = l.names(&l.trouble(&bad));
        let want: HashSet<String> = ["a", "b", "c", "h", "lone"].map(String::from).into();
        assert_eq!(got, want, "c and h are five calls apart: not joined");
        let ins: Vec<Instance> = ["a", "b", "c"]
            .iter()
            .zip([&["b", "pg"][..], &["c", "migrate"][..], &["zz"][..]])
            .map(|(n, needs)| Instance::new(n, Kind::App, needs))
            .chain([
                Instance::new("pg", Kind::Module, &[]),
                Instance::new("migrate", Kind::Task, &[]),
            ])
            .collect();
        let keep: HashSet<String> = ["a", "b"].map(String::from).into();
        let sub = subset(&ins, &keep);
        let names: Vec<&str> = sub.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "migrate"]);
        assert_eq!(sub[0].needs, ["b"]);
        assert_eq!(sub[1].needs, ["migrate"]);
    }
}
