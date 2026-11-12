//! The layered map, for stacks too large for the Go map's single grid: the calls split into their
//! connected parts, each laid out on its own (layers by longest chain of callers, crossing
//! reduction, coordinates, lines through reserved places) and packed onto shelves; the lane and
//! the band as grids below. Every call's line is clear of every card by construction, and a
//! relayout reuses every part that did not change.

mod coord;
mod order;
mod pack;
mod part;
pub(crate) mod rank;
mod route;
mod sides;
mod uses;

use std::collections::HashMap;
use std::sync::Arc;

use crate::cards::card;
use crate::classic::{back_edges, depths, split};
use crate::geom::{Pt, Rect};
use crate::index::Index;
use crate::input::{Flow, Instance};
use crate::output::{Caption, CaptionKind, CardId, Edge, Layout, Mode, Role};
use pack::{Packing, pack};
use part::{COL_GAP, Local, local, signature};

pub(crate) use uses::use_lines;

/// The content's left; the left corridor use lines run down is half of it.
pub(crate) const LEFT: f64 = 24.0;
const TOP: f64 = 16.0;
const CAP_GAP: f64 = 36.0;

/// What a layered layout keeps for the next one.
#[derive(Debug, Clone, Default)]
pub(crate) struct Memory {
    /// Each part by its signature: a part that did not change is reused as it is.
    parts: HashMap<u64, Arc<Local>>,
    packing: Option<(Packing, HashMap<String, usize>)>,
    band: Vec<String>,
}

/// How a card's use lines leave it: down the column gap beside it to its shelf's gutter, or down
/// to the gutter under its lane row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Exit {
    Flow { channel: f64, gutter: f64 },
    Lane { gutter: f64 },
    Band,
}

/// The free lines use lines run along.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Gutters {
    pub(crate) left: f64,
    pub(crate) right: f64,
    pub(crate) band: f64,
    pub(crate) exits: Vec<Exit>,
}

/// The parts placed in the drawing.
struct Placed {
    cards: Vec<(usize, Rect, Role, Exit)>,
    edges: Vec<(usize, usize, crate::geom::Path, bool)>,
    bottom: f64,
    right: f64,
}

pub(crate) fn layout(ins: &[Instance], mem: Option<&Memory>) -> (Layout, Memory) {
    let f = Flow::read(ins);
    let (nodes, lane) = split(&f);
    let back = back_edges(&f, &nodes);
    let mut depth = depths(&f, &nodes, &back);
    rank::promote(&f, &nodes, &back, &mut depth);
    let parts = rank::parts(&f, &nodes);
    // Where the earlier layout had every vertex, built only if a part changed.
    let mut seed: Option<HashMap<String, f64>> = None;
    let locals: Vec<(u64, Arc<Local>)> = parts
        .iter()
        .map(|p| {
            let sig = signature(&f, p, &depth);
            if let Some(l) = mem.and_then(|m| m.parts.get(&sig)) {
                return (sig, Arc::clone(l));
            }
            let seed = seed.get_or_insert_with(|| marks(mem));
            (sig, Arc::new(local(&f, p, &depth, &back, seed)))
        })
        .collect();
    let sizes: Vec<(f64, f64)> = locals.iter().map(|(_, l)| l.size).collect();
    let anchors: Vec<&str> = parts
        .iter()
        .map(|p| p.first().map_or("", |&v| f.name(v)))
        .collect();
    let packing = pack(&sizes, &anchors, mem.and_then(|m| m.packing.as_ref()));
    let mut caps = Vec::new();
    let mut placed = place_flow(&f, &locals, &packing);
    if !locals.is_empty() {
        let sub = match parts.len() {
            1 => "left to right, the way a request travels".to_string(),
            n => format!("left to right, the way a request travels · {n} separate parts"),
        };
        caps.push(caption(CaptionKind::Calls, "Who calls whom", &sub, TOP));
    }
    let (band, band_names) = place_sides(
        &f,
        &lane,
        &mut placed,
        &mut caps,
        mem.map(|m| m.band.as_slice()),
    );
    let memory = Memory {
        parts: locals.into_iter().collect(),
        packing: Some((
            packing,
            anchors
                .iter()
                .enumerate()
                .map(|(i, a)| ((*a).to_string(), i))
                .collect(),
        )),
        band: band_names,
    };
    (assemble(&f, placed, caps, band), memory)
}

/// Every vertex's place in the earlier layout, by key.
fn marks(mem: Option<&Memory>) -> HashMap<String, f64> {
    mem.map(|m| {
        m.parts
            .values()
            .flat_map(|l| l.marks.iter().cloned())
            .collect()
    })
    .unwrap_or_default()
}

fn caption(kind: CaptionKind, text: &str, sub: &str, y: f64) -> Caption {
    Caption {
        kind,
        text: text.into(),
        sub: sub.into(),
        at: Pt::new(LEFT, y),
    }
}

fn place_flow(f: &Flow<'_>, locals: &[(u64, Arc<Local>)], packing: &Packing) -> Placed {
    let by_name: HashMap<&str, usize> = f
        .ins
        .iter()
        .enumerate()
        .map(|(i, v)| (v.name.as_str(), i))
        .collect();
    let top = TOP + CAP_GAP;
    let mut p = Placed {
        cards: Vec::new(),
        edges: Vec::new(),
        bottom: TOP,
        right: 0.0,
    };
    for (k, (_, l)) in locals.iter().enumerate() {
        let (x, y, shelf) = packing.origin(k);
        let (ox, oy) = (x + LEFT, y + top);
        let gutter = packing.gutter(shelf) + top;
        for (name, r) in &l.cards {
            let Some(&v) = by_name.get(name.as_str()) else {
                continue;
            };
            let r = r.translate(ox, oy);
            let exit = Exit::Flow {
                channel: r.right() + COL_GAP / 2.0,
                gutter,
            };
            p.cards.push((v, r, Role::Flow, exit));
        }
        for (a, b, path, back) in &l.edges {
            if let (Some(&a), Some(&b)) = (by_name.get(a.as_str()), by_name.get(b.as_str())) {
                p.edges.push((a, b, path.translate(ox, oy), *back));
            }
        }
        p.right = p.right.max(ox + l.size.0);
        p.bottom = p.bottom.max(top + packing.height());
    }
    p
}

/// The lane and the band under the calls; gives the band's top and its order.
fn place_sides(
    f: &Flow<'_>,
    lane: &[usize],
    p: &mut Placed,
    caps: &mut Vec<Caption>,
    prev_band: Option<&[String]>,
) -> (Option<f64>, Vec<String>) {
    let width = (p.right - LEFT).max(1222.0 - 2.0 * LEFT);
    let mut y = if p.cards.is_empty() {
        TOP
    } else {
        p.bottom + 14.0
    };
    if !lane.is_empty() {
        let events = lane.iter().all(|&a| f.on_events(a));
        let (text, sub) = if events {
            ("Nobody calls these", "they work from events")
        } else {
            ("No callers", "nothing in doze.hcl refers to these")
        };
        caps.push(caption(CaptionKind::Lane, text, sub, y));
        let g = sides::lane(f, lane, y + CAP_GAP, width);
        for (v, r, gutter) in g.cards {
            p.right = p.right.max(r.right());
            p.cards.push((v, r, Role::Lane, Exit::Lane { gutter }));
        }
        y = g.bottom;
    }
    if f.shared.is_empty() {
        p.bottom = y;
        return (None, Vec::new());
    }
    let at: HashMap<usize, Rect> = p.cards.iter().map(|(v, r, _, _)| (*v, *r)).collect();
    let order = sides::band_order(f, &at, prev_band);
    let band = y + 16.0;
    let n = f.uses_all();
    let mut sub = String::from("the chips on each card say who uses what");
    if n > 1 {
        sub = format!("one line each would be {n} lines; {sub}");
    }
    caps.push(caption(
        CaptionKind::Band,
        "Shared by the stack",
        &sub,
        band + 10.0,
    ));
    let g = sides::band(f, &order, band, width);
    for (v, r, _) in g.cards {
        p.right = p.right.max(r.right());
        p.cards.push((v, r, Role::Shared, Exit::Band));
    }
    p.bottom = g.bottom;
    (
        Some(band),
        order.iter().map(|&s| f.name(s).to_string()).collect(),
    )
}

fn assemble(f: &Flow<'_>, p: Placed, captions: Vec<Caption>, band: Option<f64>) -> Layout {
    let mut ids = vec![None; f.ins.len()];
    for (i, (v, ..)) in p.cards.iter().enumerate() {
        if let Some(s) = ids.get_mut(*v) {
            *s = Some(CardId::of(i));
        }
    }
    let id = |v: usize| ids.get(v).copied().flatten();
    let cards: Vec<_> = p
        .cards
        .iter()
        .map(|&(v, r, role, _)| card(f, v, r, role, &id))
        .collect();
    let edges: Vec<Edge> = p
        .edges
        .into_iter()
        .filter_map(|(a, b, path, back)| {
            Some(Edge {
                from: id(a)?,
                to: id(b)?,
                path,
                count: 1,
                back,
            })
        })
        .collect();
    let w = (p.right + LEFT).max(1222.0);
    let h = p.bottom + 16.0;
    let gutters = Gutters {
        left: LEFT / 2.0,
        right: w - LEFT / 2.0,
        band: band.map_or(0.0, |b| b + sides::BAND_GUTTER),
        exits: p.cards.iter().map(|c| c.3).collect(),
    };
    let by_name = cards
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.clone(), CardId::of(i)))
        .collect();
    let index = Index::build(&cards, &edges);
    Layout {
        mode: Mode::Layered,
        bounds: Rect::new(0.0, 0.0, w, h),
        band,
        captions,
        cards,
        edges,
        detail: Pt::new(w - 12.0 - 448.0, 14.0),
        groups: HashMap::new(),
        lines: Vec::new(),
        gutters: Some(gutters),
        index,
        by_name,
    }
}
