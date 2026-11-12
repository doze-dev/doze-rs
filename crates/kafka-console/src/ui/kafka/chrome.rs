//! The frame parts a topic page repeats.

use maud::{Markup, html};

use crate::ui::glyph::{Glyph, glyph};

/// A tab: its label, where it goes, whether it is the one showing, and the count beside it where there is one.
#[derive(Debug, Clone)]
pub struct Tab<'a> {
    pub label: &'a str,
    pub href: String,
    pub on: bool,
    pub count: Option<usize>,
}

/// The row of tabs. The one showing says so to a screen reader as well as by its look.
#[must_use]
pub fn tabs(items: &[Tab<'_>]) -> Markup {
    html! {
        div class="tabs" {
            @for t in items {
                a class={"tab" @if t.on { " on" }} href=(t.href) aria-current=[t.on.then_some("page")] {
                    (t.label)
                    @if let Some(n) = t.count { span class="n" { (n) } }
                }
            }
        }
    }
}

/// The name of the page, in the monospace face, with the words that go beside it (a policy pill, a state).
#[must_use]
pub fn title_row(name: &str, beside: Markup) -> Markup {
    html! { div class="title-row" { h1 class="mono" { (name) } (beside) } }
}

/// An empty state: a glyph in a colour class, a heading, a paragraph and what can be done about it.
#[must_use]
pub fn empty(icon: Glyph, class: &str, extra: Option<&str>, title: Markup, body: Markup, acts: Markup) -> Markup {
    html! {
        div class="empty" data-extra=[extra] {
            span class="ic" { (glyph(icon, 22, class)) }
            // An h2: the page has only an h1 above it. The kit draws an h3 here and the kit's CSS styles both alike.
            h2 { (title) }
            p { (body) }
            (acts)
        }
    }
}

/// A section: a heading, the sentence under it, and its body. `top` is the space above it in pixels.
#[must_use]
pub fn sec(top: u8, title: &str, lede: Option<Markup>, body: Markup) -> Markup {
    html! {
        div class="sec" style=[(top > 0).then(|| format!("margin-top:{top}px"))] {
            h2 { (title) }
            @if let Some(l) = lede { p class="lede" { (l) } }
            (body)
        }
    }
}

/// The call a page makes, drawn as the boards do: a header and the request in a code block.
#[must_use]
pub fn apicall(title: &str, code: Markup, help: Option<Markup>) -> Markup {
    html! {
        div class="apicall" data-extra="api-preview" {
            div class="hd" { (glyph(Glyph::Api, 14, "")) (title) }
            pre class="code" { (code) }
            @if let Some(h) = help { p class="help" style="margin:12px 0 0" { (h) } }
        }
    }
}

/// A search box of the toolbar: a field named `name`, inside a form that is a plain GET.
#[must_use]
pub fn filter_box(name: &str, placeholder: &str, label: &str, value: &str) -> Markup {
    html! {
        div class="finput" {
            (glyph(Glyph::Search, 16, ""))
            input type="text" name=(name) placeholder=(placeholder) value=(value) aria-label=(label);
        }
    }
}

/// The line under a table: what is shown, and a button for more.
#[must_use]
pub fn pager(left: &str, button: Markup) -> Markup {
    html! { div class="pager" { span { (left) } (button) } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tab_that_is_showing_says_so_and_a_count_is_beside_the_label() {
        let m = tabs(&[
            Tab { label: "Messages", href: "/topics/a".to_owned(), on: true, count: None },
            Tab { label: "Consumers", href: "/groups".to_owned(), on: false, count: Some(2) },
        ])
        .into_string();
        assert_eq!(
            m,
            r#"<div class="tabs"><a class="tab on" href="/topics/a" aria-current="page">Messages</a><a class="tab" href="/groups">Consumers<span class="n">2</span></a></div>"#
        );
    }

    #[test]
    fn a_name_is_escaped_in_the_title_row_and_the_filter_box() {
        let t = title_row("<b>x</b>", html! {}).into_string();
        assert_eq!(t, r#"<div class="title-row"><h1 class="mono">&lt;b&gt;x&lt;/b&gt;</h1></div>"#);
        let f = filter_box("q", "Find", "Find", "\"><img onerror=1>").into_string();
        assert!(f.contains("value=\"&quot;&gt;&lt;img onerror=1&gt;\""), "{f}");
    }

    #[test]
    fn a_section_is_spaced_only_when_asked() {
        assert!(sec(0, "H", None, html! {}).into_string().starts_with(r#"<div class="sec"><h2>H</h2>"#));
        assert!(sec(32, "H", None, html! {}).into_string().contains(r#"style="margin-top:32px""#));
    }
}
