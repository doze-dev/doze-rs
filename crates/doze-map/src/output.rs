//! What a layout gives a renderer: cards placed, calls routed, captions, the bounds, and an index
//! to ask what is in view.

use std::collections::HashMap;

use crate::geom::{Path, Pt, Rect};
use crate::index::Index;
use crate::input::Kind;
use crate::text::Lang;

/// A card's place in [`Layout::cards`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CardId(pub u32);

impl CardId {
    /// The index into [`Layout::cards`].
    #[must_use]
    pub fn ix(self) -> usize {
        self.0 as usize
    }
    pub(crate) fn of(i: usize) -> Self {
        Self(u32::try_from(i).unwrap_or(u32::MAX))
    }
}

/// Where a card sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Role {
    /// In the calls, a column per step of calls.
    Flow,
    /// In the lane of apps nothing calls and that call nothing.
    Lane,
    /// In the shared band: a module or an outside service.
    Shared,
}

/// One card.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    /// The instance's name, or a collapsed group's (`worker_*`).
    pub name: String,
    /// Where it is.
    pub rect: Rect,
    /// Which part of the map it is in.
    pub role: Role,
    /// What declared it.
    pub kind: Kind,
    /// The language badge.
    pub lang: Option<Lang>,
    /// The shared services it uses that fit as chips, in doze.hcl's order.
    pub chips: Vec<String>,
    /// How many uses did not fit.
    pub more: u32,
    /// It uses nothing shared (the chip row says so instead).
    pub quiet: bool,
    /// Its setup tasks fold into a mark on it.
    pub fold: bool,
    /// How many instances it stands for: 1, or a collapsed group's size.
    pub members: u32,
    /// Its neighbours, for what a selection keeps lit.
    pub calls: Vec<CardId>,
    /// Who calls it.
    pub callers: Vec<CardId>,
    /// What it uses in the band (or, for a band card, who uses it).
    pub uses: Vec<CardId>,
}

/// One call, drawn with an arrowhead.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// The caller.
    pub from: CardId,
    /// The called.
    pub to: CardId,
    /// The line, placed to pass behind no card.
    pub path: Path,
    /// How many calls it stands for (more than one between collapsed groups).
    pub count: u32,
    /// It closes a cycle: drawn back, right to left.
    pub back: bool,
    /// It joins two named regions: a plain curve from frame to frame, drawn on demand, not kept
    /// clear of the cards between (see [`Layout::regions`]).
    pub across: bool,
}

/// Which caption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CaptionKind {
    /// Over the calls.
    Calls,
    /// Over the lane.
    Lane,
    /// Over the shared band.
    Band,
    /// Over a column of a focus view (a neighbourhood's hops).
    Column,
}

/// A caption over a part of the map: its name and a quieter line after it.
#[derive(Debug, Clone, PartialEq)]
pub struct Caption {
    /// Which part.
    pub kind: CaptionKind,
    /// The name.
    pub text: String,
    /// The quieter line.
    pub sub: String,
    /// Its top left.
    pub at: Pt,
}

/// How the map was laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Mode {
    /// The Go map's layout, card for card: small stacks.
    Classic,
    /// Layered, by connected part, packed: large stacks.
    Layered,
    /// A focus view made from another layout (a card's neighbourhood): columns by path.
    Focus,
}

/// A named region of a layered map: the cards whose names start the same (`checkout-api`,
/// `checkout-worker`), or that doze.hcl puts in one `group`, framed together.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedRegion {
    /// The name: the shared stem or the group, or `other` for the cards of no region of four.
    pub name: String,
    /// The frame: the region's cards with room around them and a head above.
    pub rect: Rect,
    /// Its cards, in the layout's order.
    pub cards: Vec<CardId>,
}

/// The cards with no calls, folded into one tile in a layered map: the apps nothing calls and
/// that call nothing, and what the stack shares (used, not called: the chips say who uses what).
/// They stay in [`Layout::cards`] (found, selected, lit) with an empty box, in no index.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tile {
    /// The folded cards, in the layout's order.
    pub cards: Vec<CardId>,
    /// How many are apps (the rest are shared).
    pub lane: u32,
    /// How many are shared modules and services.
    pub shared: u32,
}

/// A laid-out map.
#[derive(Debug, Clone)]
pub struct Layout {
    /// How it was laid out.
    pub mode: Mode,
    /// The drawing's size, from 0,0.
    pub bounds: Rect,
    /// The top of the shared band, when the stack shares anything.
    pub band: Option<f64>,
    /// The captions.
    pub captions: Vec<Caption>,
    /// Every card: the calls and the lane first, then the band.
    pub cards: Vec<Card>,
    /// Every call.
    pub edges: Vec<Edge>,
    /// Where the selected card's detail card goes, clear of the cards where there is room (in
    /// a layered map, the top right of the drawing: a renderer keeps it in the viewport).
    pub detail: Pt,
    /// Collapsed groups: the card's name, then its members' names.
    pub groups: HashMap<String, Vec<String>>,
    /// Named regions, when the map has them (a layered map of three or more regions of four).
    pub regions: Vec<NamedRegion>,
    /// Each card's named region (by [`CardId`]), `None` outside any.
    pub region_of: Vec<Option<usize>>,
    /// The folded cards, when a layered map folds them.
    pub tile: Option<Tile>,
    pub(crate) folded: Vec<bool>,
    pub(crate) lines: Vec<(CardId, CardId, Path)>,
    pub(crate) gutters: Option<crate::layered::Gutters>,
    pub(crate) index: Index,
    pub(crate) by_name: HashMap<String, CardId>,
}

impl Layout {
    /// The card named `name`, or the collapsed group card a member of that name is in.
    #[must_use]
    pub fn card_named(&self, name: &str) -> Option<CardId> {
        if let Some(&c) = self.by_name.get(name) {
            return Some(c);
        }
        self.groups
            .iter()
            .find(|(_, members)| members.iter().any(|m| m == name))
            .and_then(|(g, _)| self.by_name.get(g).copied())
    }

    /// The card.
    #[must_use]
    pub fn card(&self, id: CardId) -> Option<&Card> {
        self.cards.get(id.ix())
    }

    /// The card is folded into the tile: it has no place on the drawing.
    #[must_use]
    pub fn folded(&self, id: CardId) -> bool {
        self.folded.get(id.ix()).copied().unwrap_or(false)
    }

    /// The card's named region.
    #[must_use]
    pub fn region(&self, id: CardId) -> Option<&NamedRegion> {
        self.region_of
            .get(id.ix())
            .copied()
            .flatten()
            .and_then(|r| self.regions.get(r))
    }

    /// The named region whose frame holds `p`, if any.
    #[must_use]
    pub fn region_at(&self, p: Pt) -> Option<usize> {
        self.regions.iter().position(|r| r.rect.inside(p))
    }

    /// What is in `view`: the cards and calls a renderer must paint, in no particular order.
    #[must_use]
    pub fn visible(&self, view: Rect) -> crate::index::Visible {
        self.index.query(view)
    }

    /// The card under `p`, if any.
    #[must_use]
    pub fn hit(&self, p: Pt) -> Option<CardId> {
        self.index.hit(p)
    }

    /// The dashed lines from `card` to what it uses in the band (or from its users, for a band
    /// card), each placed to pass behind no card. Drawn only for the selection, so a layered map
    /// routes them when asked rather than for every card up front.
    #[must_use]
    pub fn use_lines(&self, card: CardId) -> Vec<(CardId, CardId, Path)> {
        let on = |a: &CardId, b: &CardId| *a == card || *b == card;
        if self.gutters.is_none() {
            return self
                .lines
                .iter()
                .filter(|(a, b, _)| on(a, b))
                .cloned()
                .collect();
        }
        crate::layered::use_lines(self, card)
    }

    /// The map at a glance: the calls between runs of cards down each column, bundled and weighted
    /// ([`crate::Overview`]), for drawing far out. Linear in cards and calls; a renderer keeps it
    /// with the layout.
    #[must_use]
    pub fn overview(&self) -> crate::Overview {
        crate::overview::overview(self)
    }

    /// The cards whose name (or a collapsed member's name) contains `q`, ignoring case: names
    /// that start with it first, then shorter names.
    #[must_use]
    pub fn search(&self, q: &str) -> Vec<CardId> {
        crate::search::search(self, q)
    }
}
