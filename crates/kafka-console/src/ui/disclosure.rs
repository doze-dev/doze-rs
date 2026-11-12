//! A disclosure: a native `<details>` that opens in place. It works with no script, never navigates, keeps the scroll where
//! it is, and opens with a short, gentle motion (`.disc` in `design/console.css`; reduced motion switches it off).
//!
//! It replaces the expanders that were GET links with a query (`?open=...`): those were a full navigation, so the page
//! jumped to the top. A deep link that wants one open (`?open=retention.ms`) is the server's way to render it `open`; the
//! toggle itself never changes the address.

use maud::{Markup, html};

/// How a disclosure looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// A link-like "Show / Hide" in the flow of the page (the explainers, a section under a table).
    Plain,
    /// A small ghost button that opens a panel under the thing it is about (a row's "Where from").
    Button,
    /// The whole header of a section is the summary: a heading and a sentence, with a Show / Hide at the right.
    Section,
}

impl Look {
    const fn class(self) -> &'static str {
        match self {
            Self::Plain => "disc",
            Self::Button => "disc disc-btn",
            Self::Section => "disc disc-sec",
        }
    }
}

/// A disclosure with an `id` (so a deep link or a same-page anchor can name it), `open` as the server renders it, the
/// summary and the body. Everything in `summary` and `body` is markup the caller built from escaped parts.
#[must_use]
pub fn disclosure(
    look: Look,
    id: Option<&str>,
    open: bool,
    summary: Markup,
    body: Markup,
) -> Markup {
    html! {
        details class=(look.class()) id=[id] open[open] {
            summary class=[(look == Look::Button).then_some("btn sm ghost")] { (summary) }
            div class="disc-body" { div class="disc-in" { (body) } }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disclosure_is_a_details_that_is_open_only_when_asked() {
        let closed = disclosure(
            Look::Plain,
            Some("why"),
            false,
            html! { "Why" },
            html! { p { "Because" } },
        )
        .into_string();
        assert!(
            closed.starts_with(r#"<details class="disc" id="why"><summary>Why</summary>"#),
            "{closed}"
        );
        assert!(!closed.contains(" open"), "{closed}");
        let open = disclosure(Look::Section, None, true, html! { "Why" }, html! {}).into_string();
        assert!(
            open.starts_with(r#"<details class="disc disc-sec" open>"#),
            "{open}"
        );
    }

    #[test]
    fn nothing_in_a_disclosure_can_navigate() {
        let m = disclosure(Look::Plain, None, false, html! { "Show" }, html! { "x" }).into_string();
        assert!(!m.contains("href"), "{m}");
    }
}
