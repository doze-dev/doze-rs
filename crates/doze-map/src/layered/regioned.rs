//! A layered map by named region ([`crate::region`]): each region laid out on its own (its own
//! calls only, by connected part, packed as the whole map's parts are), framed, and the frames
//! packed onto shelves, callers' regions first. Calls inside a region are routed as every layered
//! call is, clear of every card; a call between two regions is a plain curve from card to card,
//! drawn on demand (a renderer shows the regions' bundles at rest).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::classic::{back_edges, depths};
use crate::draw::link;
use crate::geom::Rect;
use crate::input::{Flow, Instance, list};
use crate::layered::pack::{Packing, pack, pack_to};
use crate::layered::part::{Local, local, signature};
use crate::layered::{Exit, LEFT, Memory, Placed, marks, rank};
use crate::output::Role;

/// Inside a frame, around its cards.
pub(crate) const PAD: f64 = 22.0;
/// A frame's head: room for its name on the frame's top edge.
pub(crate) const HEAD: f64 = 30.0;

/// One region laid out from 0,0.
struct Block {
    cards: Vec<(String, Rect)>,
    edges: Vec<(String, String, crate::geom::Path, bool)>,
    size: (f64, f64),
}

/// What laying the regions out leaves for the next layout.
#[derive(Debug, Clone, Default)]
pub(crate) struct Kept {
    pub(crate) parts: HashMap<u64, Arc<Local>>,
    pub(crate) packs: HashMap<String, (Packing, HashMap<String, usize>)>,
    pub(crate) frames: Option<(Packing, HashMap<String, usize>)>,
}

/// The region's instances alone: its cards, the tasks they fold, and only the references among
/// them (a call to another region is drawn across, not laid out).
fn sub(f: &Flow<'_>, members: &[usize]) -> Vec<Instance> {
    let mut take: Vec<usize> = members.to_vec();
    for &v in members {
        for &t in list(&f.tasks, v) {
            if !take.contains(&t) {
                take.push(t);
            }
        }
    }
    let names: HashSet<&str> = take.iter().map(|&v| f.name(v)).collect();
    take.iter()
        .filter_map(|&v| f.inst(v))
        .map(|i| {
            let mut i = i.clone();
            i.needs.retain(|n| names.contains(n.as_str()));
            i
        })
        .collect()
}

/// One region from 0,0: its parts laid out (or reused) and packed.
fn block(
    f: &Flow<'_>,
    name: &str,
    members: &[usize],
    mem: Option<&Memory>,
    kept: &mut Kept,
) -> Block {
    let ins = sub(f, members);
    let sf = Flow::read(&ins);
    let mut nodes = sf.apps.clone();
    nodes.extend(sf.carded_by_name());
    let back = back_edges(&sf, &nodes);
    let mut depth = depths(&sf, &nodes, &back);
    rank::promote(&sf, &nodes, &back, &mut depth);
    let parts = rank::parts(&sf, &nodes);
    let mut seed: Option<HashMap<String, f64>> = None;
    let locals: Vec<Arc<Local>> = parts
        .iter()
        .map(|p| {
            let sig = signature(&sf, p, &depth);
            let l = match mem.and_then(|m| m.parts.get(&sig)) {
                Some(l) => Arc::clone(l),
                None => {
                    let seed = seed.get_or_insert_with(|| marks(mem));
                    Arc::new(local(&sf, p, &depth, &back, seed))
                }
            };
            kept.parts.insert(sig, Arc::clone(&l));
            l
        })
        .collect();
    let sizes: Vec<(f64, f64)> = locals.iter().map(|l| l.size).collect();
    let anchors: Vec<&str> = parts
        .iter()
        .map(|p| p.first().map_or("", |&v| sf.name(v)))
        .collect();
    let prev = mem.and_then(|m| m.regions.packs.get(name));
    let packing = pack(&sizes, &anchors, prev);
    let mut b = Block {
        cards: Vec::new(),
        edges: Vec::new(),
        size: (0.0, packing.height()),
    };
    for (k, l) in locals.iter().enumerate() {
        let (x, y, _) = packing.origin(k);
        b.cards
            .extend(l.cards.iter().map(|(n, r)| (n.clone(), r.translate(x, y))));
        b.edges.extend(
            l.edges
                .iter()
                .map(|(a, c, p, bk)| (a.clone(), c.clone(), p.translate(x, y), *bk)),
        );
        b.size.0 = b.size.0.max(x + l.size.0);
    }
    let by: HashMap<String, usize> = anchors
        .iter()
        .enumerate()
        .map(|(i, a)| ((*a).to_string(), i))
        .collect();
    kept.packs.insert(name.to_string(), (packing, by));
    b
}

/// Lays the regions out under `top` and frames them: the flow's cards and calls, and each frame.
pub(crate) fn place(
    f: &Flow<'_>,
    rs: &[(String, Vec<usize>)],
    mem: Option<&Memory>,
    top: f64,
) -> (Placed, Kept) {
    let mut kept = Kept::default();
    let blocks: Vec<Block> = rs
        .iter()
        .map(|(name, vs)| block(f, name, vs, mem, &mut kept))
        .collect();
    let sizes: Vec<(f64, f64)> = blocks
        .iter()
        .map(|b| (b.size.0 + 2.0 * PAD, b.size.1 + HEAD + PAD))
        .collect();
    let anchors: Vec<&str> = rs.iter().map(|(n, _)| n.as_str()).collect();
    // About as wide as a window's map is: the whole map fits at a zoom where names still read.
    let frames = pack_to(
        &sizes,
        &anchors,
        mem.and_then(|m| m.regions.frames.as_ref()),
        2.8,
    );
    let by_name: HashMap<&str, usize> = f
        .ins
        .iter()
        .enumerate()
        .map(|(i, v)| (v.name.as_str(), i))
        .collect();
    let mut p = Placed::new(top);
    for (k, b) in blocks.iter().enumerate() {
        let (x, y, _) = frames.origin(k);
        let (fx, fy) = (x + LEFT, y + top);
        let (sw, sh) = sizes.get(k).copied().unwrap_or_default();
        let (ox, oy) = (fx + PAD, fy + HEAD);
        let mut cards = Vec::with_capacity(b.cards.len());
        for (name, r) in &b.cards {
            if let Some(&v) = by_name.get(name.as_str()) {
                p.cards
                    .push((v, r.translate(ox, oy), Role::Flow, Exit::Band));
                cards.push(v);
            }
        }
        for (a, c, path, back) in &b.edges {
            if let (Some(&a), Some(&c)) = (by_name.get(a.as_str()), by_name.get(c.as_str())) {
                p.edges.push((a, c, path.translate(ox, oy), *back, false));
            }
        }
        let name = rs.get(k).map_or("", |r| r.0.as_str());
        p.frames
            .push((name.to_string(), Rect::new(fx, fy, sw, sh), cards));
        p.right = p.right.max(fx + sw);
        p.bottom = p.bottom.max(fy + sh);
    }
    across(f, rs, &mut p);
    kept.frames = Some((
        frames,
        anchors
            .iter()
            .enumerate()
            .map(|(i, a)| ((*a).to_string(), i))
            .collect(),
    ));
    (p, kept)
}

/// The calls between regions, card to card, in region order.
fn across(f: &Flow<'_>, rs: &[(String, Vec<usize>)], p: &mut Placed) {
    let mut region: HashMap<usize, usize> = HashMap::new();
    for (i, (_, vs)) in rs.iter().enumerate() {
        for &v in vs {
            region.insert(v, i);
        }
    }
    let at: HashMap<usize, Rect> = p.cards.iter().map(|(v, r, ..)| (*v, *r)).collect();
    for (_, vs) in rs {
        for &u in vs {
            for &w in f.calls(u) {
                if region.get(&u) == region.get(&w) {
                    continue;
                }
                if let (Some(a), Some(b)) = (at.get(&u), at.get(&w)) {
                    let back = b.center().x < a.center().x;
                    p.edges.push((u, w, link(a, b), back, true));
                }
            }
        }
    }
}
