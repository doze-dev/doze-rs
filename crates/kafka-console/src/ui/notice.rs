//! Callouts, banners and empty states.

use maud::{Markup, html};

use super::glyph::{Glyph, glyph};

/// The tone of a callout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Warn,
    Bad,
    Refused,
}

impl Tone {
    const fn class(self) -> &'static str {
        match self {
            Self::Plain => "callout",
            Self::Warn => "callout warn",
            Self::Bad => "callout bad",
            Self::Refused => "callout refused",
        }
    }
}

/// A callout: a glyph, then the body.
#[must_use]
pub fn callout(tone: Tone, icon: Glyph, body: Markup) -> Markup {
    html! { div class=(tone.class()) { (glyph(icon, 18, "")) div { (body) } } }
}

/// A banner across the top of the content, in the bad tone: something is wrong with the console itself.
#[must_use]
pub fn banner_bad(id: Option<&str>, hidden: bool, body: Markup) -> Markup {
    html! { div class="banner bad" id=[id] hidden[hidden] role="status" { (body) } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_callout_has_its_tone_glyph_and_body() {
        let m = callout(Tone::Warn, Glyph::Warn, html! { b { "1 thing" } }).into_string();
        assert!(
            m.starts_with(r#"<div class="callout warn"><svg class="gl" width="18""#),
            "{m}"
        );
        assert!(m.ends_with("<div><b>1 thing</b></div></div>"), "{m}");
    }

    #[test]
    fn a_hidden_banner_is_marked_hidden() {
        let m = banner_bad(Some("x"), true, html! { "gone" }).into_string();
        assert_eq!(
            m,
            r#"<div class="banner bad" id="x" hidden role="status">gone</div>"#
        );
    }
}
