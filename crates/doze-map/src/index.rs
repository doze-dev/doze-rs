//! The spatial index: an R*-tree (rstar) over every card and every piece of every call's line,
//! so a renderer asks for what is in its viewport and paints only that, and a click finds its card
//! without a scan.

use rstar::{AABB, RTree, RTreeObject};

use crate::geom::{Pt, Rect, Seg};
use crate::output::{Card, CardId, Edge};

/// A line is indexed in pieces of this many commands, so a long line that crosses the map is not
/// one box as big as the map.
const PIECE: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum What {
    Card(u32),
    Edge(u32),
}

#[derive(Debug, Clone, Copy)]
struct Item {
    env: AABB<[f64; 2]>,
    what: What,
}

impl RTreeObject for Item {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.env
    }
}

/// What is in a viewport.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Visible {
    /// The cards that overlap it.
    pub cards: Vec<CardId>,
    /// The calls (indexes into [`crate::Layout::edges`]) with a piece that overlaps it, each once.
    pub edges: Vec<usize>,
}

/// The index of a layout.
#[derive(Debug, Clone, Default)]
pub(crate) struct Index {
    tree: RTree<Item>,
}

fn env(r: &Rect) -> AABB<[f64; 2]> {
    AABB::from_corners([r.x, r.y], [r.right(), r.bottom()])
}

impl Index {
    /// The index of `cards` (but those `skip` marks: folded, they have no place) and `edges`.
    pub(crate) fn build(cards: &[Card], skip: &[bool], edges: &[Edge]) -> Self {
        let mut items: Vec<Item> = Vec::with_capacity(cards.len() + edges.len() * 2);
        for (i, c) in cards.iter().enumerate() {
            if skip.get(i).copied().unwrap_or(false) {
                continue;
            }
            let what = What::Card(u32::try_from(i).unwrap_or(u32::MAX));
            items.push(Item {
                env: env(&c.rect),
                what,
            });
        }
        for (i, e) in edges.iter().enumerate() {
            let what = What::Edge(u32::try_from(i).unwrap_or(u32::MAX));
            items.extend(pieces(&e.path.segs).map(|env| Item { env, what }));
        }
        Index {
            tree: RTree::bulk_load(items),
        }
    }

    pub(crate) fn query(&self, view: Rect) -> Visible {
        let mut v = Visible::default();
        for it in self.tree.locate_in_envelope_intersecting(env(&view)) {
            match it.what {
                What::Card(c) => v.cards.push(CardId(c)),
                What::Edge(e) => v.edges.push(e as usize),
            }
        }
        v.edges.sort_unstable();
        v.edges.dedup();
        v
    }

    pub(crate) fn hit(&self, p: Pt) -> Option<CardId> {
        let at = AABB::from_point([p.x, p.y]);
        self.tree
            .locate_in_envelope_intersecting(at)
            .find_map(|it| match it.what {
                What::Card(c) => Some(CardId(c)),
                What::Edge(_) => None,
            })
    }
}

/// The boxes of a path's pieces: each holds `PIECE` commands, their control points included
/// (which bound the curves).
fn pieces(segs: &[Seg]) -> impl Iterator<Item = AABB<[f64; 2]>> + '_ {
    let mut prev: Option<Pt> = None;
    let starts: Vec<(Pt, &[Seg])> = segs
        .chunks(PIECE)
        .filter_map(|chunk| {
            let from = prev.or_else(|| chunk.first().map(Seg::end))?;
            prev = chunk.last().map(Seg::end);
            Some((from, chunk))
        })
        .collect();
    starts.into_iter().map(|(from, chunk)| {
        let (mut lo, mut hi) = ([from.x, from.y], [from.x, from.y]);
        for s in chunk {
            let pts: [Pt; 3] = match *s {
                Seg::Move(p) | Seg::Line(p) => [p, p, p],
                Seg::Quad(c, p) => [c, p, p],
                Seg::Cubic(a, b, p) => [a, b, p],
            };
            for p in pts {
                lo = [lo[0].min(p.x), lo[1].min(p.y)];
                hi = [hi[0].max(p.x), hi[1].max(p.y)];
            }
        }
        AABB::from_corners(lo, hi)
    })
}
