//! The glyph family of the kit (`glyphs.ts`): 16 by 16, 1.5 stroke, rounded. A name is a variant, so a glyph that is
//! not drawn does not compile. The paths are constants of this file, never data.

use maud::{Markup, PreEscaped, html};

/// The glyphs the pages use. A slice adds the ones its pages need from the kit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Home,
    Live,
    Grid,
    Access,
    Flask,
    Settings,
    Search,
    Copy,
    Code,
    Refresh,
    Check,
    X,
    Info,
    Warn,
    ArrowR,
    ChevR,
    Lock,
    Plus,
    List,
    Send,
    Trash,
    Filter,
    Topic,
    Queue,
    Api,
}

impl Glyph {
    const fn paths(self) -> &'static str {
        match self {
            Self::Home => {
                r#"<path d="M2.8 7.4 8 3l5.2 4.4"/><path d="M4 6.6V13h8V6.6"/><path d="M6.8 13v-3.2h2.4V13"/>"#
            }
            Self::Live => {
                r#"<circle cx="8" cy="8" r="1.4" fill="currentColor"/><path d="M5.1 5.3a4 4 0 0 0 0 5.4"/><path d="M10.9 5.3a4 4 0 0 1 0 5.4"/><path d="M2.9 3.5a6.9 6.9 0 0 0 0 9"/><path d="M13.1 3.5a6.9 6.9 0 0 1 0 9"/>"#
            }
            Self::Grid => {
                r#"<rect x="2.8" y="2.8" width="4.4" height="4.4" rx="1.2"/><rect x="8.8" y="2.8" width="4.4" height="4.4" rx="1.2"/><rect x="2.8" y="8.8" width="4.4" height="4.4" rx="1.2"/><rect x="8.8" y="8.8" width="4.4" height="4.4" rx="1.2"/>"#
            }
            Self::Access => {
                r#"<circle cx="5.6" cy="8" r="2.7"/><path d="M8.3 8H13.6"/><path d="M11.6 8v2"/><path d="M13.6 8v1.4"/>"#
            }
            Self::Flask => {
                r#"<path d="M6.4 2.8h3.2"/><path d="M7 2.8v4L3.7 12.3a1.1 1.1 0 0 0 1 1.7h6.6a1.1 1.1 0 0 0 1-1.7L9 6.8v-4"/><path d="M5.3 10h5.4"/>"#
            }
            Self::Settings => {
                r##"<path d="M2.8 5h10.4"/><path d="M2.8 11h10.4"/><circle cx="5.6" cy="5" r="1.6"/><circle cx="10.4" cy="11" r="1.6"/>"##
            }
            Self::Search => r#"<circle cx="7" cy="7" r="4.5"/><path d="M10.4 10.4 14 14"/>"#,
            Self::Copy => {
                r#"<rect x="5.4" y="5.4" width="7.6" height="7.6" rx="1.8"/><path d="M3 10.2V4.8A1.8 1.8 0 0 1 4.8 3H10"/>"#
            }
            Self::Code => r#"<path d="M5.4 4.6 2 8l3.4 3.4M10.6 4.6 14 8l-3.4 3.4M9 3.4 7 12.6"/>"#,
            Self::Refresh => r#"<path d="M13 8a5 5 0 1 1-1.7-3.8M13 3v3.2h-3.2"/>"#,
            Self::Check => r#"<path d="M3.4 8.6l3 3L12.6 4.6"/>"#,
            Self::X => r#"<path d="M4 4l8 8M12 4 4 12"/>"#,
            Self::Info => r#"<circle cx="8" cy="8" r="5.6"/><path d="M8 7.4v3.4M8 5.3v.1"/>"#,
            Self::Warn => r#"<path d="M8 2.6 14 13H2z"/><path d="M8 6.6v3M8 11.3v.1"/>"#,
            Self::ArrowR => r#"<path d="M2.8 8h10.4M9.4 4.2 13.2 8l-3.8 3.8"/>"#,
            Self::ChevR => r#"<path d="M6 3.5 10.5 8 6 12.5"/>"#,
            Self::Lock => {
                r#"<rect x="3.4" y="7" width="9.2" height="6.4" rx="1.8"/><path d="M5.4 7V5.4a2.6 2.6 0 0 1 5.2 0V7"/>"#
            }
            Self::Plus => r#"<path d="M8 3.2v9.6M3.2 8h9.6"/>"#,
            Self::List => {
                r#"<path d="M5.4 4h8M5.4 8h8M5.4 12h8"/><circle cx="2.8" cy="4" r=".7" fill="currentColor"/><circle cx="2.8" cy="8" r=".7" fill="currentColor"/><circle cx="2.8" cy="12" r=".7" fill="currentColor"/>"#
            }
            Self::Send => r#"<path d="M2.6 8 13.4 2.8 10.2 13.2 7.6 9z"/><path d="M7.6 9 13.4 2.8"/>"#,
            Self::Trash => r#"<path d="M3 4.6h10M6.4 4.6V3h3.2v1.6M4.4 4.6l.6 8.4h6l.6-8.4"/>"#,
            Self::Filter => r#"<path d="M2.5 3.5h11L9.6 8.4V13l-3.2-1.2V8.4z"/>"#,
            Self::Topic => {
                r#"<circle cx="3.8" cy="8" r="1.6"/><path d="M5.4 8H8.6"/><path d="M8.6 4h4M8.6 8h4M8.6 12h4"/><path d="M8.6 4v8"/>"#
            }
            Self::Queue => {
                r#"<rect x="2" y="2" width="12" height="3.2" rx="1.6"/><rect x="2" y="6.4" width="12" height="3.2" rx="1.6"/><rect x="2" y="10.8" width="12" height="3.2" rx="1.6"/>"#
            }
            Self::Api => {
                r#"<circle cx="3.8" cy="8" r="1.8"/><circle cx="12.2" cy="3.8" r="1.8"/><circle cx="12.2" cy="12.2" r="1.8"/><path d="M5.5 7.2 10.5 4.6M5.5 8.8l5 2.6"/>"#
            }
        }
    }
}

/// The glyph at `size` pixels, with an extra class (`d-data` for a domain colour, or empty).
#[must_use]
pub fn glyph(g: Glyph, size: u8, class: &str) -> Markup {
    html! {
        svg class=(format!("gl {class}").trim_end()) width=(size) height=(size) viewBox="0 0 16 16" aria-hidden="true" {
            (PreEscaped(g.paths()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glyph_is_a_decorative_svg_of_the_size_asked() {
        let s = glyph(Glyph::Check, 18, "").into_string();
        assert!(
            s.starts_with(
                r#"<svg class="gl" width="18" height="18" viewBox="0 0 16 16" aria-hidden="true">"#
            ),
            "{s}"
        );
        assert!(s.contains("M3.4 8.6l3 3L12.6 4.6"));
        assert!(
            glyph(Glyph::Code, 16, "d-data")
                .into_string()
                .contains(r#"class="gl d-data""#)
        );
    }
}
