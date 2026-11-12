//! The glyphs the Groups pages add to the family (`ui::glyph`): the same kit, 16 by 16, 1.5 stroke. In a file of their own so a
//! slice that adds glyphs elsewhere does not meet this one.

use maud::{Markup, PreEscaped, html};

/// The glyphs of the Groups pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupGlyph {
    Group,
    User,
    Back,
    Clock,
    Dot,
    ChevD,
}

impl GroupGlyph {
    const fn paths(self) -> &'static str {
        match self {
            Self::Group => {
                r#"<circle cx="5.8" cy="5.6" r="2.2"/><circle cx="11" cy="6.4" r="1.8"/><path d="M1.8 13c.5-2.3 2.2-3.4 4-3.4s3.5 1.1 4 3.4"/><path d="M10 9.8c1.7 0 3.2 1 3.7 3"/>"#
            }
            Self::User => {
                r#"<circle cx="8" cy="5.4" r="2.5"/><path d="M3 13.6c.6-2.6 2.7-4 5-4s4.4 1.4 5 4"/>"#
            }
            Self::Back => r#"<path d="M13.2 8H2.8M6.6 4.2 2.8 8l3.8 3.8"/>"#,
            Self::Clock => r#"<circle cx="8" cy="8" r="5.6"/><path d="M8 4.8v3.4l2.1 1.3"/>"#,
            Self::Dot => r#"<circle cx="8" cy="8" r="3" fill="currentColor"/>"#,
            Self::ChevD => r#"<path d="M3.5 6 8 10.5 12.5 6"/>"#,
        }
    }
}

/// The glyph at `size` pixels, with an extra class (`dim`, or empty).
#[must_use]
pub fn group_glyph(g: GroupGlyph, size: u8, class: &str) -> Markup {
    html! {
        svg class=(format!("gl {class}").trim_end()) width=(size) height=(size) viewBox="0 0 16 16" aria-hidden="true" { (PreEscaped(g.paths())) }
    }
}
