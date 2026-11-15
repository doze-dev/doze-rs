//! A layered map's dashed use lines, routed when a card is selected: out of the card into the
//! channel beside its column (or under its lane row), down to its shelf's gutter, along it to the
//! nearer corridor at the drawing's edge, down to the band's gutter, along it to the gap beside the
//! band card, down to the gap above it, and in. Every leg runs where no card can be, so the line is
//! clear by construction; it is long, but drawn only for the selection.

use crate::draw::elbow_h;
use crate::geom::{Path, Pt, Rect};
use crate::layered::part::COL_GAP;
use crate::layered::sides::BAND_DY;
use crate::layered::{Exit, Gutters};
use crate::output::{CardId, Layout, Role};

/// The use lines of `card`: to what it uses, or from its users for a band card.
pub(crate) fn use_lines(l: &Layout, card: CardId) -> Vec<(CardId, CardId, Path)> {
    let (Some(g), Some(c)) = (l.gutters.as_ref(), l.card(card)) else {
        return Vec::new();
    };
    let pairs: Vec<(CardId, CardId)> = if c.role == Role::Shared {
        c.uses.iter().map(|&u| (u, card)).collect()
    } else {
        c.uses.iter().map(|&s| (card, s)).collect()
    };
    pairs
        .into_iter()
        .filter_map(|(a, b)| {
            let (s, t) = (l.card(a)?.rect, l.card(b)?.rect);
            let exit = g.exits.get(a.ix()).copied()?;
            Some((a, b, line(g, exit, &s, &t)))
        })
        .collect()
}

fn line(g: &Gutters, exit: Exit, s: &Rect, t: &Rect) -> Path {
    let mut pts = Vec::with_capacity(10);
    let gutter = match exit {
        Exit::Flow { channel, gutter } => {
            let y0 = (s.center().y + 18.0).min(s.bottom() - 4.0);
            pts.extend([
                Pt::new(s.right(), y0),
                Pt::new(channel, y0),
                Pt::new(channel, gutter),
            ]);
            gutter
        }
        Exit::Lane { gutter } => {
            pts.extend([
                Pt::new(s.center().x, s.bottom()),
                Pt::new(s.center().x, gutter),
            ]);
            gutter
        }
        Exit::Band => {
            return Path {
                segs: Vec::new(),
                end: crate::geom::End::Dot,
            };
        }
    };
    let left = s.center().x < f64::midpoint(g.left, g.right);
    let corridor = if left { g.left } else { g.right };
    // The gap beside the band card on the corridor's side; for the first or last column, the
    // corridor itself.
    let side = if left {
        (t.x - COL_GAP / 2.0).max(g.left)
    } else {
        (t.right() + COL_GAP / 2.0).min(g.right)
    };
    let above = t.y - BAND_DY / 2.0;
    pts.extend([
        Pt::new(corridor, gutter),
        Pt::new(corridor, g.band),
        Pt::new(side, g.band),
        Pt::new(side, above),
        Pt::new(t.center().x, above),
        Pt::new(t.center().x, t.y),
    ]);
    elbow_h(0.0, &pts).path
}
