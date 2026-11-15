//! One connected part of the calls, laid out on its own from 0,0: layers, crossing reduction,
//! coordinates, lines. Kept by its signature, so a relayout where the part did not change reuses
//! it, card for card.

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::geom::{Path, Rect};
use crate::input::Flow;
use crate::layered::coord::{CARD_H, assign};
use crate::layered::order::reduce;
use crate::layered::rank::build;
use crate::layered::route::{Geo, route};
use crate::text::{CARD_W, Lang, card_width};

/// Between columns: room for a line to bend and an arrowhead to land.
pub(crate) const COL_GAP: f64 = 72.0;
/// Below a line's placeholder, the room a part keeps.
const LINE_ROOM: f64 = 10.0;

/// A part laid out from 0,0, by name so it outlives the input it came from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Local {
    pub(crate) cards: Vec<(String, Rect)>,
    pub(crate) edges: Vec<(String, String, Path, bool)>,
    /// The part's size, room for one more column included when a card in its last column has
    /// setup tasks (a task that starts running gets a card there, and the part keeps its place).
    pub(crate) size: (f64, f64),
    /// Every vertex's centre y by key (a card's name; a placeholder's call and column), which a
    /// relayout of the part starts from.
    pub(crate) marks: Vec<(String, f64)>,
}

/// What a part's layout depends on: its cards, in order, their widths and their calls.
pub(crate) fn signature(f: &Flow<'_>, members: &[usize], depth: &[usize]) -> u64 {
    let mut h = DefaultHasher::new();
    for &v in members {
        f.name(v).hash(&mut h);
        width(f, v).hash(&mut h);
        depth.get(v).hash(&mut h);
        f.folds(v).hash(&mut h);
        for &w in f.calls(v) {
            f.name(w).hash(&mut h);
        }
        0xffu8.hash(&mut h);
    }
    h.finish()
}

pub(crate) fn width(f: &Flow<'_>, v: usize) -> i64 {
    let run = f.inst(v).map_or("", |i| i.run.as_str());
    card_width(f.name(v), Lang::of(run))
}

/// Lays the part out; `seed` holds an earlier layout's centre y by name, which a changed part
/// keeps as far as its new shape allows.
pub(crate) fn local(
    f: &Flow<'_>,
    members: &[usize],
    depth: &[usize],
    back: &HashSet<(usize, usize)>,
    seed: &HashMap<String, f64>,
) -> Local {
    let mut lg = build(f, members, depth, back);
    let keys = keys(f, &lg);
    let seeds: Vec<Option<f64>> = keys.iter().map(|k| seed.get(k).copied()).collect();
    let seed_of = |v: usize| seeds.get(v).copied().flatten();
    reduce(&mut lg, &seed_of);
    let y = assign(&lg, &seed_of);
    let mut col_w: Vec<f64> = vec![CARD_W as f64; lg.layers.len()];
    for (v, r) in lg.real.iter().enumerate() {
        if let (Some(i), Some(&l)) = (r, lg.layer.get(v))
            && let Some(w) = col_w.get_mut(l)
        {
            *w = w.max(width(f, *i) as f64);
        }
    }
    let mut col_x = Vec::with_capacity(col_w.len());
    let mut x = 0.0;
    for w in &col_w {
        col_x.push(x);
        x += w + COL_GAP;
    }
    let geo = Geo { col_x, col_w, y };
    let cards: Vec<(String, Rect)> = lg
        .real
        .iter()
        .enumerate()
        .filter_map(|(v, r)| r.map(|i| (f.name(i).to_string(), geo.rect(&lg, v, CARD_H))))
        .collect();
    let edges = route(&lg, &geo, CARD_H)
        .into_iter()
        .map(|(a, b, p, back)| (f.name(a).to_string(), f.name(b).to_string(), p, back))
        .collect();
    let last = lg.layers.len().saturating_sub(1);
    let grows = lg
        .real
        .iter()
        .enumerate()
        .any(|(v, r)| lg.layer.get(v) == Some(&last) && r.is_some_and(|i| f.folds(i)));
    let mut w = geo.col_x.last().copied().unwrap_or(0.0) + geo.col_w.last().copied().unwrap_or(0.0);
    if grows {
        w += COL_GAP + CARD_W as f64;
    }
    // The lowest card or line: a placeholder can sit below every card.
    let lines = geo.y.iter().map(|y| y + LINE_ROOM).fold(0.0, f64::max);
    let h = cards.iter().map(|(_, r)| r.bottom()).fold(lines, f64::max);
    let marks = keys.into_iter().zip(geo.y.iter().copied()).collect();
    Local {
        cards,
        edges,
        size: (w, h),
        marks,
    }
}

/// Each vertex's key: a card's name; a placeholder's caller, column and direction (its trunk).
fn keys(f: &Flow<'_>, lg: &crate::layered::rank::Lg) -> Vec<String> {
    let mut keys: Vec<String> = lg
        .real
        .iter()
        .map(|r| r.map(|i| f.name(i).to_string()).unwrap_or_default())
        .collect();
    for ch in &lg.chains {
        for &d in ch
            .verts
            .get(1..ch.verts.len().saturating_sub(1))
            .unwrap_or(&[])
        {
            let layer = lg.layer.get(d).copied().unwrap_or(0);
            if let Some(k) = keys.get_mut(d) {
                *k = format!("{}\u{0}{layer}\u{0}{}", f.name(ch.from), ch.back);
            }
        }
    }
    keys
}
