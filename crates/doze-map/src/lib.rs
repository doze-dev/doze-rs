//! The desktop app's Map: a stack laid out as cards, the calls between them and captions, for a
//! renderer to paint. No GUI here: positions, lines and an index, from instances in.
//!
//! # Two layouts
//!
//! A small stack gets the Go map's layout (`internal/desk/view/flow.go`), card for card: the apps
//! left to right the way a request travels, a column per step of calls; the apps nothing calls in
//! a lane; what the stack shares in a band below, with a chip on each app card for what it uses;
//! lines placed to pass behind no card; dashed use lines for the selection. Its Go golden and three
//! hundred stacks Go laid out are this crate's oracle (`tests/oracle.rs`).
//!
//! That layout lays everything on one grid and routes every line by trying shapes against every
//! card, which is right for twenty cards and wrong for two thousand. Past
//! [`Options::classic_max`] cards a stack gets the layered layout:
//!
//! - the calls split into their connected parts, each laid out on its own (most real stacks are
//!   many teams' services, loosely joined), then packed onto shelves;
//! - in a part, each card's column is its longest chain of callers (the Go rule), a call that skips
//!   columns gets a reserved place in each column it crosses, and the order down each column is
//!   chosen to cross little: barycentre sweeps, crossings counted exactly, the best kept;
//! - positions down a column solve a weighted isotonic regression per column (pool adjacent
//!   violators), so calls run as straight as the spacing allows;
//! - lines are built through the reserved places, so none passes behind a card, by construction;
//!   use lines are routed only when asked ([`Layout::use_lines`]), along gutters kept free;
//! - groups (an instance's `group`, or the `name_suffix` instances `count` and `for_each` stamp)
//!   collapse into one card each past [`Collapse::over`] instances, or when asked;
//! - an R*-tree over cards and line pieces answers what is in a viewport ([`Layout::visible`]) and
//!   what is under the pointer ([`Layout::hit`]);
//! - far out, [`Layout::overview`] bundles the calls between runs of cards down each column, so a
//!   renderer draws a few weighted lines instead of thousands of single ones;
//! - a [`Mapper`] keeps the last layout: a change of state alone moves nothing and costs a hash;
//!   a change inside one part relays only that part, and it keeps its place on its shelf when it
//!   still fits there (a part keeps room for a task card after its last column).
//!
//! # Medium stacks: named regions, the tile, focus views
//!
//! Past [`Options::classic_max`], a stack that splits into three or more named regions of four
//! cards or more ([`Options::regions`]; a region is an instance's `group`, else its name's stem,
//! and a task card joins its caller's) is laid out by region: each region on its own calls, framed
//! ([`Layout::regions`]), the frames packed callers first and about a window wide. A call between
//! two regions is a plain curve ([`Edge::across`]), not kept clear of cards: a renderer draws the
//! regions' bundles ([`Layout::overview`]) at rest and those calls on demand. The lane and the band
//! fold into one tile ([`Layout::tile`]): still cards (found, selected), with no place on the drawing.
//! Two focus views are made from a layout: [`Layout::neighbourhood`] (columns by path) and
//! [`Layout::trouble`] with [`subset`] (laid out on their own). [`synth::medium`] makes stacks of
//! 20 to 150 shaped like real ones. Medians on the same machine, 2026-11-13 (`cargo bench`):
//! 100 services and 333 calls by region, 0.46 ms; 150 and 500, 0.57 ms (the budget is 20 ms);
//! 2,000 synthetic, now by region too, 12.3 ms.
//!
//! # Prior art, and what was taken from it
//!
//! Measured on this machine, 2,000 nodes and 5,000 edges (release build; `rust-sugiyama` 0.4.0
//! and `layout-rs` 0.1.3 run from a scratch crate):
//!
//! | | 100 / 250 | 1,000 / 2,500 | 2,000 / 5,000 |
//! |---|---|---|---|
//! | rust-sugiyama (network simplex, barycentre, Brandes–Köpf) | 2.4 ms | 1.37 s | 18.9 s |
//! | layout-rs (Graphviz-style, renders SVG) | 83 ms | 21.8 s | 113 s |
//! | this crate (layered, every line routed and indexed) | 0.55 ms | 6.6 ms | 16.8 ms |
//!
//! Neither is usable at the target size, neither routes edges clear of nodes for a renderer
//! (rust-sugiyama gives coordinates only; layout-rs draws SVG), neither keeps the Go map's rules,
//! and neither relays incrementally. From dagre the crate takes placeholders for long edges and
//! laying out connected parts separately; from ELK's layered algorithm, model order as the tie
//! breaker (what keeps a relayout stable) and packing parts. Spatial index: `rstar`, the georust
//! R*-tree (MIT/Apache, no required dependencies), bulk-loaded; a uniform grid would do for cards
//! but not for lines that cross the map.
//!
//! # Budgets
//!
//! `cargo bench -p doze-map` (benches/layout.rs) prints the table and fails when a budget is
//! broken: a full layered layout of 2,000 nodes under 50 ms, a viewport query under 1 ms, a
//! relayout after a change of state under 5 ms, after one task card appears under 20 ms. Synthetic
//! stacks from [`synth::stack`]: `edges` are calls between apps (uses come on top, one to three
//! per app), every group expanded unless the row says otherwise. A viewport is 1280 × 800 at zoom
//! 1, centred on a card (about 200 cards and lines in view at 2,000). Medians, release build,
//! Apple M-series laptop, 2026-11-13:
//!
//! | nodes / calls | cards | mode | full layout | viewport query | whole-map query | state change | one task card appears | cards it moved |
//! |---|---|---|---|---|---|---|---|---|
//! | 10 / 25 | 10 | classic | 0.09 ms | 0.4 µs | 0.6 µs | 1 µs | – | – |
//! | 100 / 250 | 95 | layered | 0.55 ms | 2.2 µs | 6.3 µs | 12 µs | 0.49 ms | 2 of 95 |
//! | 1,000 / 2,500 | 948 | layered | 6.6 ms | 4.7 µs | 65 µs | 117 µs | 4.6 ms | 3 of 948 |
//! | 2,000 / 5,000 | 1,893 | layered | 16.8 ms | 6.1 µs | 183 µs | 341 µs | 12.0 ms | 0 of 1,893 |
//! | 5,000 / 12,500 | 4,728 | layered | 82 ms | 8.0 µs | 1.07 ms | 0.79 ms | 47.7 ms | 27 of 4,728 |
//! | 2,000 / 5,000, groups collapsed | 1,466 | layered | 20.1 ms | 6.0 µs | 177 µs | 280 µs | 14.3 ms | 9 of 1,466 |
//!
//! On a loaded machine (load average 52, other builds running) the same run measured 2,000 nodes at
//! 30 ms and the one-part relayout at 20 ms: the budgets hold with about half to spare when the
//! machine is quiet, not when it is saturated. 5,000 nodes is past the target and degrades
//! linearly; the cost there is one connected part of 3,900 cards (crossing reduction on 43,000
//! vertices), which a real stack of that size would usually not have.

mod cards;
mod classic;
mod draw;
mod focus;
mod geom;
mod group;
mod index;
mod input;
mod layered;
mod output;
mod overview;
mod region;
mod search;
pub mod synth;
mod text;
mod trouble;

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

pub use draw::HEAD_LEN;
pub use focus::MAX_HOPS;
pub use geom::{End, Path, Pt, Rect, Seg};
pub use group::{Collapse, Grouping};
pub use index::Visible;
pub use input::{Instance, Kind};
pub use output::{Caption, CaptionKind, Card, CardId, Edge, Layout, Mode, NamedRegion, Role, Tile};
pub use overview::{Bundle, Link, Overview, Region};
pub use text::Lang;
pub use trouble::subset;

/// How to lay a stack out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Up to this many cards, the Go map's layout; past it, the layered one.
    pub classic_max: usize,
    /// Where groups come from.
    pub grouping: Grouping,
    /// When groups collapse.
    pub collapse: Collapse,
    /// Past [`Options::classic_max`], a stack that splits into three or more named regions of
    /// four or more cards is laid out by region, each framed, and the cards with no calls (the
    /// lane and the band) fold into one tile. Off, the layered map draws its lane and band.
    pub regions: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            classic_max: 60,
            grouping: Grouping::default(),
            collapse: Collapse::default(),
            regions: true,
        }
    }
}

/// Lays `ins` (in display order) out once. To relayout as the stack changes, keep a [`Mapper`].
#[must_use]
pub fn layout(ins: &[Instance], opts: &Options) -> Layout {
    let mut m = Mapper::new(opts.clone());
    Arc::unwrap_or_clone(m.layout(ins))
}

/// Lays a stack out again and again as it changes, moving as little as it can.
#[derive(Debug, Default)]
pub struct Mapper {
    opts: Options,
    memory: Option<layered::Memory>,
    last: Option<(u64, Arc<Layout>)>,
}

impl Mapper {
    /// A mapper with no layout yet.
    #[must_use]
    pub fn new(opts: Options) -> Self {
        Mapper {
            opts,
            memory: None,
            last: None,
        }
    }

    /// The options.
    #[must_use]
    pub fn options(&self) -> &Options {
        &self.opts
    }

    /// Changes the options (to open or close a group, say); the next layout uses them.
    pub fn set_options(&mut self, opts: Options) {
        self.opts = opts;
        self.last = None;
    }

    /// The layout of `ins`. When nothing the layout depends on changed (a state change: awake,
    /// asleep, its memory), it is the last one, unmoved.
    pub fn layout(&mut self, ins: &[Instance]) -> Arc<Layout> {
        let key = structure(ins);
        if let Some((k, l)) = &self.last
            && *k == key
        {
            return Arc::clone(l);
        }
        let c = group::collapse(ins, self.opts.grouping, &self.opts.collapse);
        let flat = c.as_ref().map_or(ins, |c| c.ins.as_slice());
        let cards = flat
            .iter()
            .filter(|v| v.kind != Kind::Task || v.busy)
            .count();
        let mut l = if cards <= self.opts.classic_max {
            self.memory = None;
            classic::layout(flat)
        } else {
            let (l, m) = layered::layout(flat, self.memory.as_ref(), self.opts.regions);
            self.memory = Some(m);
            l
        };
        if let Some(c) = c {
            for e in &mut l.edges {
                let (a, b) = (l.cards.get(e.from.ix()), l.cards.get(e.to.ix()));
                if let (Some(a), Some(b)) = (a, b) {
                    e.count = c
                        .counts
                        .get(&(a.name.clone(), b.name.clone()))
                        .copied()
                        .unwrap_or(1);
                }
            }
            l.groups = c.groups;
        }
        let l = Arc::new(l);
        self.last = Some((key, Arc::clone(&l)));
        l
    }
}

/// What a layout depends on: names, kinds, references, badges, the lane's caption and which tasks
/// are cards. Not state, not memory.
fn structure(ins: &[Instance]) -> u64 {
    let mut h = DefaultHasher::new();
    for v in ins {
        v.name.hash(&mut h);
        v.kind.hash(&mut h);
        v.needs.hash(&mut h);
        Lang::of(&v.run).hash(&mut h);
        v.engine.hash(&mut h);
        v.busy.hash(&mut h);
        v.group.hash(&mut h);
        v.members.hash(&mut h);
    }
    h.finish()
}
