//! The placed parts as a [`Layout`]: cards with their ids, calls, the frames of named regions, the
//! folded tile, the bounds, the use lines' gutters and the index.

use std::collections::HashMap;

use crate::cards::card;
use crate::geom::{Pt, Rect};
use crate::index::Index;
use crate::input::Flow;
use crate::layered::{Gutters, LEFT, Placed, sides};
use crate::output::{Caption, CardId, Edge, Layout, Mode, NamedRegion, Role, Tile};

pub(crate) fn assemble(
    f: &Flow<'_>,
    p: Placed,
    captions: Vec<Caption>,
    band: Option<f64>,
) -> Layout {
    let placed: Vec<(usize, Rect, Role)> = p
        .cards
        .iter()
        .map(|&(v, r, role, _)| (v, r, role))
        .chain(p.folded.iter().map(|&(v, role)| (v, Rect::default(), role)))
        .collect();
    let mut ids = vec![None; f.ins.len()];
    for (i, (v, ..)) in placed.iter().enumerate() {
        if let Some(s) = ids.get_mut(*v) {
            *s = Some(CardId::of(i));
        }
    }
    let id = |v: usize| ids.get(v).copied().flatten();
    let cards: Vec<_> = placed
        .iter()
        .map(|&(v, r, role)| card(f, v, r, role, &id))
        .collect();
    let edges: Vec<Edge> = p
        .edges
        .into_iter()
        .filter_map(|(a, b, path, back, across)| {
            Some(Edge {
                from: id(a)?,
                to: id(b)?,
                path,
                count: 1,
                back,
                across,
            })
        })
        .collect();
    let w = (p.right + LEFT).max(1222.0);
    let h = p.bottom + 16.0;
    let flow = p.cards.len();
    let folded: Vec<bool> = (0..cards.len()).map(|i| i >= flow).collect();
    let tile = tile(&p.folded, flow);
    // Folded, the band has no cards to route use lines to.
    let gutters = tile.is_none().then(|| Gutters {
        left: LEFT / 2.0,
        right: w - LEFT / 2.0,
        band: band.map_or(0.0, |b| b + sides::BAND_GUTTER),
        exits: p.cards.iter().map(|c| c.3).collect(),
    });
    let (regions, region_of) = frames(p.frames, cards.len(), &id);
    let by_name = cards
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.clone(), CardId::of(i)))
        .collect();
    let index = Index::build(&cards, &folded, &edges);
    Layout {
        mode: Mode::Layered,
        bounds: Rect::new(0.0, 0.0, w, h),
        band,
        captions,
        cards,
        edges,
        detail: Pt::new(w - 12.0 - 448.0, 14.0),
        groups: HashMap::new(),
        regions,
        region_of,
        tile,
        folded,
        lines: Vec::new(),
        gutters,
        index,
        by_name,
    }
}

/// The tile, when anything folds: the folded cards come after the `flow` placed ones.
fn tile(folded: &[(usize, Role)], flow: usize) -> Option<Tile> {
    let count = |role: Role| {
        u32::try_from(folded.iter().filter(|(_, r)| *r == role).count()).unwrap_or(u32::MAX)
    };
    (!folded.is_empty()).then(|| Tile {
        cards: (flow..flow + folded.len()).map(CardId::of).collect(),
        lane: count(Role::Lane),
        shared: count(Role::Shared),
    })
}

/// The named regions' frames, with their cards' ids, and each card's region.
fn frames(
    frames: Vec<(String, Rect, Vec<usize>)>,
    n: usize,
    id: &dyn Fn(usize) -> Option<CardId>,
) -> (Vec<NamedRegion>, Vec<Option<usize>>) {
    let mut region_of = vec![None; n];
    let regions = frames
        .into_iter()
        .enumerate()
        .map(|(k, (name, rect, vs))| {
            let cards: Vec<CardId> = vs.into_iter().filter_map(id).collect();
            for c in &cards {
                if let Some(s) = region_of.get_mut(c.ix()) {
                    *s = Some(k);
                }
            }
            NamedRegion { name, rect, cards }
        })
        .collect();
    (regions, region_of)
}
