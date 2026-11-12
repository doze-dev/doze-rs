//! The glyphs the Log and Producers pages add to the family (`ui::glyph`): the same kit, 16 by 16, 1.5 stroke. In a file of
//! their own so a slice that adds glyphs elsewhere does not meet this one.

use maud::{Markup, PreEscaped, html};

/// The glyphs of the Log pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogGlyph {
    Eye,
    Log,
    ChevD,
}

impl LogGlyph {
    const fn paths(self) -> &'static str {
        match self {
            Self::Eye => {
                r#"<path d="M1.8 8s2.4-4.4 6.2-4.4S14.2 8 14.2 8s-2.4 4.4-6.2 4.4S1.8 8 1.8 8z"/><circle cx="8" cy="8" r="1.9"/>"#
            }
            Self::Log => {
                r#"<rect x="3" y="2" width="10" height="12" rx="1.8"/><path d="M5.5 5.4h5M5.5 8h5M5.5 10.6h3"/>"#
            }
            Self::ChevD => r#"<path d="M3.5 6 8 10.5 12.5 6"/>"#,
        }
    }
}

/// The glyph at `size` pixels, with an extra class (`dim`, or empty).
#[must_use]
pub fn log_glyph(g: LogGlyph, size: u8, class: &str) -> Markup {
    html! {
        svg class=(format!("gl {class}").trim_end()) width=(size) height=(size) viewBox="0 0 16 16" aria-hidden="true" { (PreEscaped(g.paths())) }
    }
}
