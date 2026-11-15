//! The placed boxes as a [`Layout`]: cards, the calls routed among them, and every use's dashed
//! line routed up front (a small map draws them all for the selection at no cost).

use std::collections::HashMap;

use crate::cards::card;
use crate::classic::{Bx, Router};
use crate::geom::{Path, Pt, Rect};
use crate::index::Index;
use crate::input::Flow;
use crate::output::{Caption, CardId, Edge, Layout, Mode, Role};

pub(crate) struct Parts<'f, 'a> {
    pub(crate) f: &'f Flow<'a>,
    pub(crate) placed: Vec<(usize, Role)>,
    pub(crate) at: Vec<Option<Bx>>,
    pub(crate) caps: Vec<Caption>,
    pub(crate) band: Option<i64>,
    pub(crate) size: (i64, i64),
    pub(crate) detail: Pt,
}

impl Parts<'_, '_> {
    pub(crate) fn build(self) -> Layout {
        let f = self.f;
        let mut ids = vec![None; f.ins.len()];
        for (i, (v, _)) in self.placed.iter().enumerate() {
            if let Some(slot) = ids.get_mut(*v) {
                *slot = Some(CardId::of(i));
            }
        }
        let id = |v: usize| ids.get(v).copied().flatten();
        let bx = |v: usize| self.at.get(v).copied().flatten();
        let cards: Vec<_> = self
            .placed
            .iter()
            .filter_map(|&(v, role)| Some(card(f, v, bx(v)?.rect(), role, &id)))
            .collect();
        let (edges, lines) = self.route(&id);
        let by_name: HashMap<String, CardId> = cards
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.clone(), CardId::of(i)))
            .collect();
        let index = Index::build(&cards, &[], &edges);
        let region_of = vec![None; cards.len()];
        Layout {
            mode: Mode::Classic,
            bounds: Rect::new(0.0, 0.0, self.size.0 as f64, self.size.1 as f64),
            band: self.band.map(|b| b as f64),
            captions: self.caps,
            cards,
            edges,
            detail: self.detail,
            groups: HashMap::new(),
            region_of,
            regions: Vec::new(),
            tile: None,
            folded: Vec::new(),
            lines,
            gutters: None,
            index,
            by_name,
        }
    }

    /// Every call, and every use's dashed line, routed among the cards in the Go map's order.
    fn route(
        &self,
        id: &dyn Fn(usize) -> Option<CardId>,
    ) -> (Vec<Edge>, Vec<(CardId, CardId, Path)>) {
        let f = self.f;
        let bx = |v: usize| self.at.get(v).copied().flatten();
        let boxes: Vec<Bx> = self.placed.iter().filter_map(|&(v, _)| bx(v)).collect();
        let mut router = Router::new(&boxes);
        let nodes: Vec<usize> = self
            .placed
            .iter()
            .filter(|(_, r)| *r != Role::Shared)
            .map(|(v, _)| *v)
            .collect();
        let mut edges = Vec::new();
        for &v in &nodes {
            let (Some(s), Some(from)) = (bx(v), id(v)) else {
                continue;
            };
            let mut targets: Vec<usize> = f.calls(v).to_vec();
            targets.sort_by_key(|&t| bx(t).map_or(0, |b| b.x));
            for t in targets {
                let (Some(tb), Some(to)) = (bx(t), id(t)) else {
                    continue;
                };
                let path = router.call(v, &s, &tb).path;
                edges.push(Edge {
                    from,
                    to,
                    path,
                    count: 1,
                    back: tb.x <= s.x,
                    across: false,
                });
            }
        }
        let mut lines = Vec::new();
        for &v in &nodes {
            let (Some(s), Some(from)) = (bx(v), id(v)) else {
                continue;
            };
            for &u in f.uses(v) {
                let (Some(tb), Some(to)) = (bx(u), id(u)) else {
                    continue;
                };
                lines.push((from, to, router.use_line(&s, &tb).path));
            }
        }
        (edges, lines)
    }
}
