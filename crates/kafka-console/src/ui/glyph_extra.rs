//! The glyphs the topic forms add to the family (`ui::glyph`): the same kit, 16 by 16, 1.5 stroke. Kept in a file of
//! their own so a slice that adds glyphs elsewhere does not meet this one.

use maud::{Markup, PreEscaped, html};

/// The extra glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extra {
    Api,
    ChevD,
    Trash,
    Send,
    List,
}

impl Extra {
    const fn paths(self) -> &'static str {
        match self {
            Self::Api => {
                r#"<circle cx="3.8" cy="8" r="1.8"/><circle cx="12.2" cy="3.8" r="1.8"/><circle cx="12.2" cy="12.2" r="1.8"/><path d="M5.5 7.2 10.5 4.6M5.5 8.8l5 2.6"/>"#
            }
            Self::ChevD => r#"<path d="M3.5 6 8 10.5 12.5 6"/>"#,
            Self::Trash => r#"<path d="M3 4.6h10M6.4 4.6V3h3.2v1.6M4.4 4.6l.6 8.4h6l.6-8.4"/>"#,
            Self::Send => {
                r#"<path d="M2.6 8 13.4 2.8 10.2 13.2 7.6 9z"/><path d="M7.6 9 13.4 2.8"/>"#
            }
            Self::List => {
                r#"<path d="M5.4 4h8M5.4 8h8M5.4 12h8"/><circle cx="2.8" cy="4" r=".7" fill="currentColor"/><circle cx="2.8" cy="8" r=".7" fill="currentColor"/><circle cx="2.8" cy="12" r=".7" fill="currentColor"/>"#
            }
        }
    }
}

/// The glyph at `size` pixels.
#[must_use]
pub fn glyph_extra(g: Extra, size: u8) -> Markup {
    html! {
        svg class="gl" width=(size) height=(size) viewBox="0 0 16 16" aria-hidden="true" { (PreEscaped(g.paths())) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glyph_is_a_decorative_svg_and_each_has_a_path() {
        let s = glyph_extra(Extra::Send, 16).into_string();
        assert!(
            s.starts_with(
                r#"<svg class="gl" width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">"#
            ),
            "{s}"
        );
        for g in [
            Extra::Api,
            Extra::ChevD,
            Extra::Trash,
            Extra::Send,
            Extra::List,
        ] {
            assert!(glyph_extra(g, 14).into_string().contains("<path") || g == Extra::Api);
        }
    }
}
