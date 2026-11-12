//! Buttons, status words and pills. Variants are enums: `Variant::Primary`, not the string `"pri"`.

use maud::{Markup, html};

use super::glyph::{Glyph, glyph};

/// How loud a button is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Plain,
    Primary,
    Danger,
}

/// How big.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Normal,
    Small,
}

/// The `class` of a button, without the glyph: `btn pri sm`.
#[must_use]
pub fn class(variant: Variant, size: Size) -> String {
    let v = match variant {
        Variant::Plain => "",
        Variant::Primary => " pri",
        Variant::Danger => " danger",
    };
    let s = match size {
        Size::Normal => "",
        Size::Small => " sm",
    };
    format!("btn{v}{s}")
}

/// A link that looks like a button, with a glyph before its label.
#[must_use]
pub fn link(href: &str, variant: Variant, size: Size, icon: Option<Glyph>, label: &str) -> Markup {
    html! {
        a class=(class(variant, size)) href=(href) {
            @if let Some(g) = icon { (glyph(g, 16, "")) }
            (label)
        }
    }
}

/// The kinds of state a word can be in (`st ok`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Ok,
    Warn,
    Bad,
    Refused,
    Info,
    Idle,
}

impl State {
    /// The class, `ok`, `warn`, ...
    #[must_use]
    pub const fn class(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Bad => "bad",
            Self::Refused => "refused",
            Self::Info => "info",
            Self::Idle => "idle",
        }
    }
}

/// A status word with its dot: `Reachable`. Colour is never the only carrier: the word is always there.
#[must_use]
pub fn state(s: State, word: &str) -> Markup {
    html! { span class={"st " (s.class())} { (word) } }
}

/// A small label in the monospace face (`go`, `c · py`).
#[must_use]
pub fn pill_mono(text: &str) -> Markup {
    html! { span class="pill mono" { (text) } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_class_is_built_from_its_variant_and_size() {
        assert_eq!(class(Variant::Plain, Size::Normal), "btn");
        assert_eq!(class(Variant::Primary, Size::Normal), "btn pri");
        assert_eq!(class(Variant::Danger, Size::Small), "btn danger sm");
    }

    #[test]
    fn a_state_is_a_word_with_a_class() {
        assert_eq!(
            state(State::Ok, "Reachable").into_string(),
            r#"<span class="st ok">Reachable</span>"#
        );
        for s in [
            State::Ok,
            State::Warn,
            State::Bad,
            State::Refused,
            State::Info,
            State::Idle,
        ] {
            assert!(!s.class().is_empty());
        }
    }

    #[test]
    fn a_link_button_escapes_its_label_and_target() {
        let m = link("/a?b=\"c\"", Variant::Plain, Size::Normal, None, "<b>x</b>").into_string();
        assert_eq!(
            m,
            r#"<a class="btn" href="/a?b=&quot;c&quot;">&lt;b&gt;x&lt;/b&gt;</a>"#
        );
    }
}
