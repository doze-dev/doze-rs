//! The glyphs the Settings and Access pages add to the family (`ui::glyph`): the same kit, 16 by 16, 1.5 stroke. In a file of
//! their own so a slice that adds glyphs elsewhere does not meet this one.

use maud::{Markup, PreEscaped, html};

/// The glyphs of the Settings and Access pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsGlyph {
    /// A pencil.
    Edit,
    /// An eye.
    Eye,
}

impl SettingsGlyph {
    const fn paths(self) -> &'static str {
        match self {
            Self::Edit => {
                r#"<path d="M3 13l.7-3.1 7.4-7.4a1.5 1.5 0 0 1 2.1 2.1L5.8 12 3 13z"/><path d="M9.9 3.7l2.1 2.1"/>"#
            }
            Self::Eye => {
                r#"<path d="M1.8 8S4 3.8 8 3.8 14.2 8 14.2 8 12 12.2 8 12.2 1.8 8 1.8 8z"/><circle cx="8" cy="8" r="1.9"/>"#
            }
        }
    }
}

/// One glyph at `size` pixels.
#[must_use]
pub fn settings_glyph(g: SettingsGlyph, size: u8) -> Markup {
    html! {
        svg class="gl" width=(size) height=(size) viewBox="0 0 16 16" aria-hidden="true" { (PreEscaped(g.paths())) }
    }
}
