//! The glyphs the Live pages add to the family (`ui::glyph`): the same kit, 16 by 16, 1.5 stroke. A file of their own, so a
//! slice that adds glyphs elsewhere does not meet this one.

use maud::{Markup, PreEscaped, html};

/// The glyphs of Live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveGlyph {
    Network,
    Play,
    Stop,
    Clock,
    ChevL,
    ChevR,
    ChevD,
    Back,
    Edit,
    Flask,
}

impl LiveGlyph {
    const fn paths(self) -> &'static str {
        match self {
            Self::Network => {
                r#"<circle cx="8" cy="3.6" r="1.6"/><circle cx="3.6" cy="12" r="1.6"/><circle cx="12.4" cy="12" r="1.6"/><path d="M7.2 5 4.4 10.6M8.8 5l2.8 5.6M5.2 12h5.6"/>"#
            }
            Self::Play => r#"<path d="M5.2 3.4v9.2l7-4.6z"/>"#,
            Self::Stop => r#"<rect x="4" y="4" width="8" height="8" rx="1.6"/>"#,
            Self::Clock => r#"<circle cx="8" cy="8" r="5.6"/><path d="M8 4.8v3.4l2.1 1.3"/>"#,
            Self::ChevL => r#"<path d="M10 3.5 5.5 8l4.5 4.5"/>"#,
            Self::ChevR => r#"<path d="M6 3.5 10.5 8 6 12.5"/>"#,
            Self::ChevD => r#"<path d="M3.5 6 8 10.5 12.5 6"/>"#,
            Self::Back => r#"<path d="M13.2 8H2.8M6.6 4.2 2.8 8l3.8 3.8"/>"#,
            Self::Edit => r#"<path d="M3 13l.7-3.1 7.3-7.3 2.4 2.4-7.3 7.3z"/>"#,
            Self::Flask => {
                r#"<path d="M6.4 2.8h3.2M7 2.8v3.4L3.6 12a1.2 1.2 0 0 0 1 1.8h6.8a1.2 1.2 0 0 0 1-1.8L9 6.2V2.8"/>"#
            }
        }
    }
}

/// A decorative glyph of the given size.
#[must_use]
pub fn live_glyph(g: LiveGlyph, size: u8) -> Markup {
    html! {
        svg class="gl" width=(size) height=(size) viewBox="0 0 16 16" aria-hidden="true" { (PreEscaped(g.paths())) }
    }
}
