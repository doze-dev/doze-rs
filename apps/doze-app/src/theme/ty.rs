//! The type: every size, line height and weight the app draws text in, from the Go app's page (`tokens.css` and
//! `shell.css`). Nothing else in the app names a raw size or weight; `tests::no_raw_type` holds it to that.
//!
//! The faces are Inter and JetBrains Mono (assets.rs), drawn as WebKit draws them under `-webkit-font-smoothing:
//! antialiased`, which the Go page sets: grayscale coverage, no Core Graphics smoothing (vendor/gpui-pre-apple
//! PATCHES.md §2).

use gpui::FontWeight;

// ── sizes, px ── `--text-*` where Go has a token, else the literal its rule uses.

/// `.tag` (NEW, last time).
pub(crate) const TAG: f32 = 9.;
/// Small mono keys in ⌘K's and Needs you's chips, `.lang`, `.box`.
pub(crate) const MICRO: f32 = 10.;
/// `--text-xs`: uppercase headings, small keys, ages.
pub(crate) const XS: f32 = 11.;
/// Notes, mono errors, Needs you's lines.
pub(crate) const XS_UP: f32 = 11.5;
/// `--text-sm`: buttons, pills, mono values, captions, the keys bar.
pub(crate) const SM: f32 = 12.;
/// Tile feet, the grid, ⌘K's rows, help's rows.
pub(crate) const SM_UP: f32 = 12.5;
/// `--text-base`: the page's body.
pub(crate) const BASE: f32 = 13.;
/// The grid's row names.
pub(crate) const BASE_UP: f32 = 13.5;
/// `--text-md`: row and tile names.
pub(crate) const MD: f32 = 14.;
/// Inspector and card titles, ⌘K's box.
pub(crate) const MD_UP: f32 = 15.;
/// `--text-lg`: dialog titles.
pub(crate) const LG: f32 = 16.;
/// The empty page's heading, Needs you's calm ring.
pub(crate) const LG_UP: f32 = 18.;
/// `--text-xl`: a page's heading.
pub(crate) const XL: f32 = 20.;
/// `--text-figure`: big numbers.
pub(crate) const FIGURE: f32 = 22.;
/// The Library's wordmark.
pub(crate) const BRAND: f32 = 40.;

// ── line heights, px ──

pub(crate) const LH_13: f32 = 13.;
pub(crate) const LH_14: f32 = 14.;
pub(crate) const LH_15: f32 = 15.;
pub(crate) const LH_16: f32 = 16.;
pub(crate) const LH_17: f32 = 17.;
pub(crate) const LH_18: f32 = 18.;
pub(crate) const LH_19: f32 = 19.;
/// `--lh`: the page's line.
pub(crate) const LH: f32 = 20.;
pub(crate) const LH_21: f32 = 21.;
pub(crate) const LH_26: f32 = 26.;
pub(crate) const LH_44: f32 = 44.;
/// A key chip's line is its glyph's size and this.
pub(crate) const CHIP_LEAD: f32 = 5.;
/// A one-line label set solid but for this (the minimap's names).
pub(crate) const TIGHT_LEAD: f32 = 2.;
/// A state mark's glyph (✗, ⊘, ✓, ▸) is set this much over the mark's size, so it reads as big as the discs beside it.
pub(crate) const MARK_GLYPH_UP: f32 = 4.;
/// And on a line this much over the mark's size.
pub(crate) const MARK_GLYPH_LEAD: f32 = 2.;
/// A key's ⏎ or ⌫, in the UI face, is set this much over the mono letters beside it.
pub(crate) const KEY_GLYPH_UP: f32 = 2.;

// ── weights ── the faces are static (400, 500, 600, 700). Go's variable Inter takes 650 and 750; a static face
// cannot, and asked for 650 GPUI picks 700 (CSS matching rounds up past 500), so those two take the lighter neighbour.

/// Body, values, captions: Go's 400.
pub(crate) const REGULAR: FontWeight = FontWeight(400.);
/// Buttons, segments, kbd in a button, column heads: Go's 500.
pub(crate) const MEDIUM: FontWeight = FontWeight(500.);
/// Names in rows, state words, `b`, key chips, small uppercase headings: Go's 600.
pub(crate) const SEMIBOLD: FontWeight = FontWeight(600.);
/// Page, card, dialog and tile headings: Go's 650, drawn at 600.
pub(crate) const HEADING: FontWeight = FontWeight(600.);
/// Group captions (`.gl`, `.qh`), tags, badges: Go's 700.
pub(crate) const BOLD: FontWeight = FontWeight(700.);
/// The wordmark: Go's 750, drawn at 700.
pub(crate) const BRAND_WEIGHT: FontWeight = FontWeight(700.);

#[cfg(test)]
mod tests;
