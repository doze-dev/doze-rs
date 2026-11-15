//! The lane and the band in a layered map: grids under the calls, wrapped to the calls' width,
//! with gutters between their rows and columns that use lines run along.

use std::collections::HashMap;

use crate::geom::Rect;
use crate::input::Flow;
use crate::layered::LEFT;
use crate::layered::coord::CARD_H;
use crate::layered::part::{COL_GAP, width};
use crate::text::CARD_W;

const LANE_DY: f64 = 40.0;
pub(crate) const SHARE_H: f64 = 88.0;
pub(crate) const BAND_DY: f64 = 28.0;
/// The band's first row sits this far under its top; its gutter halfway up that.
const BAND_ROWS: f64 = 44.0;
pub(crate) const BAND_GUTTER: f64 = BAND_ROWS - BAND_DY / 2.0;
const PER_ROW: usize = 5;

/// A grid's cards and, for each, the gutter under its row.
#[derive(Debug, Clone, Default)]
pub(crate) struct Grid {
    pub(crate) cards: Vec<(usize, Rect, f64)>,
    pub(crate) bottom: f64,
}

fn per_row(width: f64, pitch: f64) -> usize {
    // Bounded by the drawing's width: the cast is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = ((width + COL_GAP) / pitch).floor().max(0.0) as usize;
    n.max(PER_ROW)
}

fn grid(items: &[usize], w: f64, h: f64, dy: f64, top: f64, width: f64) -> Grid {
    let pitch = w + COL_GAP;
    let per = per_row(width, pitch);
    let mut g = Grid {
        cards: Vec::with_capacity(items.len()),
        bottom: top,
    };
    for (i, &v) in items.iter().enumerate() {
        let (r, c) = ((i / per) as f64, (i % per) as f64);
        let rect = Rect::new(LEFT + c * pitch, top + r * (h + dy), w, h);
        g.bottom = g.bottom.max(rect.bottom());
        g.cards.push((v, rect, rect.bottom() + dy / 2.0));
    }
    g
}

/// The lane's cards from `top`, wrapped at `width`.
pub(crate) fn lane(f: &Flow<'_>, lane: &[usize], top: f64, width: f64) -> Grid {
    let w = lane
        .iter()
        .map(|&v| width_of(f, v))
        .fold(CARD_W as f64, f64::max);
    grid(lane, w, CARD_H, LANE_DY, top, width)
}

fn width_of(f: &Flow<'_>, v: usize) -> f64 {
    width(f, v) as f64
}

/// The band's order: under the cards that use it (the mean of their centres), unless an earlier
/// layout gave one, which is kept, with what is new after it.
pub(crate) fn band_order(
    f: &Flow<'_>,
    at: &HashMap<usize, Rect>,
    prev: Option<&[String]>,
) -> Vec<usize> {
    let pull = |s: usize| {
        let users = f.users(s);
        let xs: Vec<f64> = users
            .iter()
            .filter_map(|u| at.get(u))
            .map(|r| r.center().x)
            .collect();
        if xs.is_empty() {
            1e12
        } else {
            xs.iter().sum::<f64>() / xs.len() as f64
        }
    };
    let mut keyed: Vec<(f64, usize)> = f.shared.iter().map(|&s| (pull(s), s)).collect();
    keyed.sort_by(|(pa, a), (pb, b)| pa.total_cmp(pb).then_with(|| f.name(*a).cmp(f.name(*b))));
    let fresh: Vec<usize> = keyed.into_iter().map(|(_, s)| s).collect();
    let Some(prev) = prev else { return fresh };
    let rank: HashMap<&str, usize> = prev
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();
    let mut kept: Vec<usize> = fresh
        .iter()
        .copied()
        .filter(|&s| rank.contains_key(f.name(s)))
        .collect();
    kept.sort_by_key(|&s| rank.get(f.name(s)).copied().unwrap_or(usize::MAX));
    kept.extend(fresh.into_iter().filter(|&s| !rank.contains_key(f.name(s))));
    kept
}

/// The band's cards under its top, wrapped at `width`.
pub(crate) fn band(f: &Flow<'_>, order: &[usize], band_top: f64, width: f64) -> Grid {
    let w = order
        .iter()
        .map(|&v| width_of(f, v))
        .fold(CARD_W as f64, f64::max);
    grid(order, w, SHARE_H, BAND_DY, band_top + BAND_ROWS, width)
}
