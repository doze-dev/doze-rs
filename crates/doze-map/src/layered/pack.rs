//! The parts packed onto shelves, largest first, left to right, a new shelf when one is full; the
//! shelves' width aims at a drawing about half again as wide as it is tall. A relayout keeps every
//! part that still fits where it was, and moves only the parts that changed and no longer fit,
//! onto a shelf of their own at the bottom: nothing that did not change moves.

use std::collections::HashMap;

/// Between parts on a shelf: room for a use line's channel beside the last column.
pub(crate) const PART_GAP: f64 = 96.0;
/// Between shelves: the gutter use lines run along.
pub(crate) const SHELF_GAP: f64 = 56.0;
/// Below this share of the flow's area in cards, a relayout repacks from scratch.
const MIN_FILL: f64 = 0.25;

/// A shelf: its top and height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shelf {
    pub(crate) y: f64,
    pub(crate) h: f64,
}

/// Where the parts went, in the flow's own coordinates (0,0 at its top left).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Packing {
    pub(crate) shelves: Vec<Shelf>,
    /// Each part's left and shelf.
    pub(crate) at: Vec<(f64, usize)>,
}

impl Packing {
    pub(crate) fn origin(&self, part: usize) -> (f64, f64, usize) {
        let (x, s) = self.at.get(part).copied().unwrap_or((0.0, 0));
        (x, self.shelves.get(s).map_or(0.0, |sh| sh.y), s)
    }
    pub(crate) fn height(&self) -> f64 {
        self.shelves.last().map_or(0.0, |s| s.y + s.h)
    }
    pub(crate) fn gutter(&self, shelf: usize) -> f64 {
        self.shelves
            .get(shelf)
            .map_or(0.0, |s| s.y + s.h + SHELF_GAP / 2.0)
    }
}

/// Packs parts of the given sizes. With an earlier packing (`prev`, by each part's anchor name),
/// a part whose anchor was placed before keeps its place when it still fits there.
pub(crate) fn pack(
    sizes: &[(f64, f64)],
    anchors: &[&str],
    prev: Option<&(Packing, HashMap<String, usize>)>,
) -> Packing {
    pack_to(sizes, anchors, prev, 1.5)
}

/// [`pack`], aiming at a drawing `aspect` times as wide as it is tall.
pub(crate) fn pack_to(
    sizes: &[(f64, f64)],
    anchors: &[&str],
    prev: Option<&(Packing, HashMap<String, usize>)>,
    aspect: f64,
) -> Packing {
    if let Some((p, by_anchor)) = prev {
        let area: f64 = sizes.iter().map(|(w, h)| w * h).sum();
        let kept = keep(sizes, anchors, p, by_anchor, aspect);
        let width = sizes
            .iter()
            .zip(&kept.at)
            .map(|((w, _), (x, _))| x + w)
            .fold(0.0, f64::max);
        if area >= MIN_FILL * width * kept.height().max(1.0) {
            return kept;
        }
    }
    fresh(sizes, aspect)
}

fn target_width(sizes: &[(f64, f64)], aspect: f64) -> f64 {
    let area: f64 = sizes
        .iter()
        .map(|(w, h)| (w + PART_GAP) * (h + SHELF_GAP))
        .sum();
    let widest = sizes.iter().map(|s| s.0).fold(0.0, f64::max);
    widest.max((area * aspect).sqrt())
}

fn fresh(sizes: &[(f64, f64)], aspect: f64) -> Packing {
    let mut p = Packing::default();
    let all: Vec<usize> = (0..sizes.len()).collect();
    p.at = vec![(0.0, 0); sizes.len()];
    shelve(&mut p, sizes, &all, target_width(sizes, aspect), 0.0);
    p
}

/// Places `which` parts on new shelves from `top` down.
fn shelve(p: &mut Packing, sizes: &[(f64, f64)], which: &[usize], width: f64, top: f64) {
    let mut x = 0.0;
    let mut shelf: Option<usize> = None;
    for &i in which {
        let (w, h) = sizes.get(i).copied().unwrap_or((0.0, 0.0));
        let s = match shelf {
            Some(s) if x == 0.0 || x + w <= width => s,
            _ => {
                let y = p
                    .shelves
                    .last()
                    .map_or(top, |l| (l.y + l.h + SHELF_GAP).max(top));
                p.shelves.push(Shelf { y, h: 0.0 });
                x = 0.0;
                p.shelves.len() - 1
            }
        };
        shelf = Some(s);
        if let Some(sh) = p.shelves.get_mut(s) {
            sh.h = sh.h.max(h);
        }
        if let Some(a) = p.at.get_mut(i) {
            *a = (x, s);
        }
        x += w + PART_GAP;
    }
}

/// The earlier packing, parts that fit kept in place; the rest on new shelves below.
fn keep(
    sizes: &[(f64, f64)],
    anchors: &[&str],
    prev: &Packing,
    by_anchor: &HashMap<String, usize>,
    aspect: f64,
) -> Packing {
    let mut p = Packing {
        shelves: prev.shelves.clone(),
        at: vec![(0.0, 0); sizes.len()],
    };
    let mut taken: Vec<(f64, f64, f64, f64)> = Vec::new();
    let mut moved = Vec::new();
    for (i, (&(w, h), anchor)) in sizes.iter().zip(anchors).enumerate() {
        let old = by_anchor
            .get(*anchor)
            .and_then(|&k| prev.at.get(k))
            .copied();
        let fits = old.and_then(|(x, s)| {
            let sh = p.shelves.get(s)?;
            let free = !taken.iter().any(|&(tx, ty, tw, th)| {
                x < tx + tw + PART_GAP && tx < x + w + PART_GAP && sh.y < ty + th && ty < sh.y + h
            });
            (h <= sh.h && free).then_some((x, s, sh.y))
        });
        match fits {
            Some((x, s, y)) => {
                taken.push((x, y, w, h));
                if let Some(a) = p.at.get_mut(i) {
                    *a = (x, s);
                }
            }
            None => moved.push(i),
        }
    }
    if !moved.is_empty() {
        let top = p.height() + SHELF_GAP;
        let width = target_width(sizes, aspect);
        shelve(&mut p, sizes, &moved, width, top);
    }
    p
}
