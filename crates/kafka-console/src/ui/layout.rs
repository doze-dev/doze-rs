//! Page structure: the header, cards, rows, key-value lists, grids.

use maud::{Markup, html};

/// The title of a page, its sentence and its actions. `sub` and `acts` are markup so a sentence can carry bold and
/// monospace; the pages build them from spans, never from data as HTML.
#[must_use]
pub fn page_header(title: &str, sub: Markup, acts: Markup) -> Markup {
    html! {
        div class="page-h" {
            div { h1 { (title) } (sub) }
            div class="acts" { (acts) }
        }
    }
}

/// A card.
#[must_use]
pub fn card(body: Markup) -> Markup {
    html! { div class="card" { (body) } }
}

/// A card that carries a `data-extra` label of the boards (`endpoint`, `live-sentence`, ...).
#[must_use]
pub fn card_extra(extra: &str, body: Markup) -> Markup {
    html! { div class="card" data-extra=(extra) { (body) } }
}

/// A card's title row: the title, and something at the right (a link, a state word).
#[must_use]
pub fn title_row(title: &str, right: Markup) -> Markup {
    html! { div class="row sb" { h2 { (title) } (right) } }
}

/// A row of key and value pairs. A value is markup; a row's second line is its `sub`.
#[must_use]
pub fn kv(rows: Vec<(&str, Markup)>) -> Markup {
    html! {
        dl class="kv sm" {
            @for (key, value) in &rows { dt { (key) } dd { (value) } }
        }
    }
}

/// A value with a small second line under it.
#[must_use]
pub fn value_with_sub(value: Markup, sub: &str) -> Markup {
    html! { (value) span class="sub" { (sub) } }
}

/// The horizontal rule between the parts of a card.
#[must_use]
pub fn hr() -> Markup {
    html! { div class="hr" {} }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kv_pairs_every_key_with_its_value() {
        let m = kv(vec![
            ("Started", html! { span class="mono" { "Nov 6" } }),
            ("Threads", html! { "3" }),
        ])
        .into_string();
        assert_eq!(
            m,
            r#"<dl class="kv sm"><dt>Started</dt><dd><span class="mono">Nov 6</span></dd><dt>Threads</dt><dd>3</dd></dl>"#
        );
    }
}
