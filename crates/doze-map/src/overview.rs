//! The whole map at a glance: the calls between neighbourhoods of cards, bundled, for a renderer
//! zoomed out too far to show single calls (a 2,000-card stack at fit is a few percent).
//!
//! A region is a cell of a grid laid over the calls: about eight cells across and as many down
//! as keep them square, each holding the cards whose centres fall in it (a cell with none is not
//! a region). Far out, that is what the eye sees as a clump of blocks; the crossing reduction has
//! already put what calls what near each other, so a region is mostly neighbours. A bundle is
//! every call from one region to another, with how many it stands for; a renderer draws it as one
//! line as wide as its weight and fades or drops the single calls, keeping a hovered or selected
//! card's own.
//!
//! The lane and the band are not regions: nothing calls lane cards, and use lines are drawn only
//! for a selection. A map laid out by named region ([`crate::NamedRegion`]) has its frames as its
//! regions instead of grid cells.

use std::collections::HashMap;

use crate::geom::{End, Path, Pt, Rect, Seg};
use crate::output::{CardId, Layout, Role};

/// Cells across the grid.
const ACROSS: f64 = 8.0;
/// At most this many rows, however tall the map.
const MAX_DOWN: f64 = 16.0;

/// A cell of cards.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    /// Its box: the cards' boxes together.
    pub rect: Rect,
    /// Its cards, in the layout's order.
    pub cards: Vec<CardId>,
    /// The name most of its cards start with (`billing` for `billing-api`, `billing-worker`), when
    /// three in five share it.
    pub label: Option<String>,
}

/// Every call from one region to another.
#[derive(Debug, Clone, PartialEq)]
pub struct Bundle {
    /// The callers' region (an index into [`Overview::regions`]).
    pub from: usize,
    /// The called region.
    pub to: usize,
    /// How many calls it stands for (a collapsed group's calls count as many).
    pub count: u32,
    /// The line: right side to left side, or left to right for calls back up the flow.
    pub path: Path,
}

/// The map at a glance.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overview {
    /// The regions.
    pub regions: Vec<Region>,
    /// Each card's region (by [`CardId`]), `None` for the lane and the band.
    pub region_of: Vec<Option<usize>>,
    /// The bundles, heaviest first.
    pub bundles: Vec<Bundle>,
    /// Calls inside one region (not bundled; a renderer may leave them out far out).
    pub inside: u32,
}

impl Overview {
    /// The heaviest bundle's count, for scaling widths (1 when there is none).
    #[must_use]
    pub fn heaviest(&self) -> u32 {
        self.bundles.first().map_or(1, |b| b.count.max(1))
    }
}

pub(crate) fn overview(l: &Layout) -> Overview {
    let (regions, region_of) = if l.regions.is_empty() {
        regions(l)
    } else {
        named(l)
    };
    let mut weights: HashMap<(usize, usize), u32> = HashMap::new();
    let mut inside = 0u32;
    for e in &l.edges {
        let at = |c: CardId| region_of.get(c.ix()).copied().flatten();
        let (Some(a), Some(b)) = (at(e.from), at(e.to)) else {
            continue;
        };
        let n = e.count.max(1);
        if a == b {
            inside = inside.saturating_add(n);
        } else {
            let w = weights.entry((a, b)).or_default();
            *w = w.saturating_add(n);
        }
    }
    let mut bundles: Vec<Bundle> = weights
        .into_iter()
        .filter_map(|((from, to), count)| {
            let path = bundle_path(&regions.get(from)?.rect, &regions.get(to)?.rect);
            Some(Bundle {
                from,
                to,
                count,
                path,
            })
        })
        .collect();
    bundles.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then((a.from, a.to).cmp(&(b.from, b.to)))
    });
    Overview {
        regions,
        region_of,
        bundles,
        inside,
    }
}

/// A map by named region: its frames are the regions.
fn named(l: &Layout) -> (Vec<Region>, Vec<Option<usize>>) {
    let regions = l
        .regions
        .iter()
        .map(|r| Region {
            rect: r.rect,
            cards: r.cards.clone(),
            label: Some(r.name.clone()),
        })
        .collect();
    (regions, l.region_of.clone())
}

/// The flow cards in grid cells, row by row.
fn regions(l: &Layout) -> (Vec<Region>, Vec<Option<usize>>) {
    let flow: Vec<(CardId, Pt)> = l
        .cards
        .iter()
        .enumerate()
        .filter(|(_, c)| c.role == Role::Flow)
        .map(|(i, c)| (CardId::of(i), c.rect.center()))
        .collect();
    let mut region_of = vec![None; l.cards.len()];
    let Some(&(_, first)) = flow.first() else {
        return (Vec::new(), region_of);
    };
    let (mut lo, mut hi) = (first, first);
    for &(_, p) in &flow {
        lo = Pt::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Pt::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    let (w, h) = ((hi.x - lo.x).max(1.0), (hi.y - lo.y).max(1.0));
    let cell = w / ACROSS;
    let down = (h / cell).ceil().clamp(1.0, MAX_DOWN);
    let cell_h = h / down;
    // A centre on the far edge belongs to the last cell, not one past it.
    let at = |v: f64, size: f64, n: f64| -> usize {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i = (v / size).floor().clamp(0.0, n - 1.0) as usize;
        i
    };
    let mut cells: std::collections::BTreeMap<(usize, usize), Vec<CardId>> =
        std::collections::BTreeMap::new();
    for &(id, p) in &flow {
        let key = (at(p.y - lo.y, cell_h, down), at(p.x - lo.x, cell, ACROSS));
        cells.entry(key).or_default().push(id);
    }
    let mut out = Vec::with_capacity(cells.len());
    for cards in cells.into_values() {
        for c in &cards {
            if let Some(slot) = region_of.get_mut(c.ix()) {
                *slot = Some(out.len());
            }
        }
        out.extend(region(l, cards));
    }
    (out, region_of)
}

fn region(l: &Layout, cards: Vec<CardId>) -> Option<Region> {
    let mut rects = cards.iter().filter_map(|c| l.card(*c)).map(|c| c.rect);
    let first = rects.next()?;
    let rect = rects.fold(first, |a, r| a.union(&r));
    let label = label(
        cards
            .iter()
            .filter_map(|c| l.card(*c))
            .map(|c| c.name.as_str()),
    );
    Some(Region { rect, cards, label })
}

/// The first word of a name: up to its first `-`, `_` or `.`.
fn stem(name: &str) -> &str {
    name.split(['-', '_', '.']).next().unwrap_or(name)
}

fn label<'a>(names: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let mut total = 0usize;
    for n in names {
        let s = stem(n);
        // A name with no separator has no stem to share.
        if s.len() < n.len() {
            *counts.entry(s).or_default() += 1;
        }
        total += 1;
    }
    let (best, n) = counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))?;
    (n >= 2 && n * 5 >= total * 3).then(|| best.to_owned())
}

/// One cubic from `a` to `b` between their nearest sides: right into left when `b` is to the right (left into
/// right when to the left); when the two overlap across, bottom into top (or top into bottom), leaving and landing
/// square.
fn bundle_path(a: &Rect, b: &Rect) -> Path {
    let (ca, cb) = (a.center(), b.center());
    let (start, end, d1, d2) = if b.x >= a.right() {
        (
            Pt::new(a.right(), ca.y),
            Pt::new(b.x, cb.y),
            (1.0, 0.0),
            (-1.0, 0.0),
        )
    } else if b.right() <= a.x {
        (
            Pt::new(a.x, ca.y),
            Pt::new(b.right(), cb.y),
            (-1.0, 0.0),
            (1.0, 0.0),
        )
    } else if cb.y >= ca.y {
        let x = f64::midpoint(a.x.max(b.x), a.right().min(b.right()));
        (
            Pt::new(x, a.bottom()),
            Pt::new(x, b.y),
            (0.0, 1.0),
            (0.0, -1.0),
        )
    } else {
        let x = f64::midpoint(a.x.max(b.x), a.right().min(b.right()));
        (
            Pt::new(x, a.y),
            Pt::new(x, b.bottom()),
            (0.0, -1.0),
            (0.0, 1.0),
        )
    };
    let pull = (start.x - end.x).abs().max((start.y - end.y).abs()) / 2.0;
    let pull = pull.max(40.0);
    Path {
        segs: vec![
            Seg::Move(start),
            Seg::Cubic(
                Pt::new(start.x + d1.0 * pull, start.y + d1.1 * pull),
                Pt::new(end.x + d2.0 * pull, end.y + d2.1 * pull),
                end,
            ),
        ],
        end: End::Arrow,
    }
}

#[cfg(test)]
mod tests {
    use super::{label, stem};

    #[test]
    fn a_label_is_a_stem_most_names_share() {
        assert_eq!(stem("billing-api"), "billing");
        assert_eq!(stem("api"), "api");
        let names = ["billing-api", "billing-worker", "billing-cron", "ads-web"];
        assert_eq!(label(names.into_iter()).as_deref(), Some("billing"));
        assert_eq!(
            label(["billing-api", "ads-web", "core-x"].into_iter()),
            None
        );
        assert_eq!(label(["api", "web", "db"].into_iter()), None);
    }
}
