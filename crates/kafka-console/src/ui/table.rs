//! Tables: a header of column names and rows of cells that are markup (so a cell can hold a link or a state word), and the
//! middle ellipsis for a long key or name in a cell.
//!
//! The header of every `table.t` stays under the top bar while a long table scrolls (`position: sticky` in
//! `design/console.css`; the corners are drawn by the cells, not by clipping the table, which would make the table the
//! scroll container of its own header).

use maud::{Markup, html};

/// A column: its name, and whether its cells are right-aligned (numbers).
#[derive(Debug, Clone, Copy)]
pub struct Column<'a> {
    pub name: &'a str,
    pub right: bool,
}

/// A table with scoped headers. An empty body renders the header alone: the empty state is the page's to draw.
#[must_use]
pub fn table(columns: &[Column<'_>], rows: Vec<Vec<Markup>>) -> Markup {
    html! {
        table class="t" {
            thead { tr { @for c in columns { th scope="col" class=[c.right.then_some("r")] { (c.name) } } } }
            tbody {
                @for row in &rows {
                    tr { @for (i, cell) in row.iter().enumerate() {
                        td class=[columns.get(i).is_some_and(|c| c.right).then_some("r")] { (cell) }
                    } }
                }
            }
        }
    }
}

/// A text longer than this many characters is cut in the middle.
pub const MIDDLE_FROM: usize = 28;
/// How many characters of the end are always shown: the end of a key or a name is its useful part.
const TAIL: usize = 10;

/// A long text in a cell: the start gives way to an ellipsis when the cell is narrow and the last ten characters always
/// stay (`orders-2026-11-1…ab3f9c01d2`). The full text is in the page twice over: split in two spans that sit side by side
/// (so selecting and copying the cell copies all of it, and a screen reader reads all of it) and in `title` (the hover).
/// A short text is returned as it is. The text is escaped; nothing here trusts it.
#[must_use]
pub fn middle(text: &str) -> Markup {
    let n = text.chars().count();
    if n <= MIDDLE_FROM {
        return html! { (text) };
    }
    let at = text
        .char_indices()
        .nth(n - TAIL)
        .map_or(text.len(), |(i, _)| i);
    let (head, tail) = text.split_at(at);
    html! { span class="mid" title=(text) { span class="mid-a" { (head) } span class="mid-b" { (tail) } } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_scopes_its_headers_and_aligns_numbers_right() {
        let cols = [
            Column {
                name: "Topic",
                right: false,
            },
            Column {
                name: "Records",
                right: true,
            },
        ];
        let m = table(&cols, vec![vec![html! { "<orders>" }, html! { "16,104" }]]).into_string();
        assert_eq!(
            m,
            r#"<table class="t"><thead><tr><th scope="col">Topic</th><th scope="col" class="r">Records</th></tr></thead><tbody><tr><td>&lt;orders&gt;</td><td class="r">16,104</td></tr></tbody></table>"#
        );
    }

    #[test]
    fn a_short_text_is_left_alone_and_a_long_one_is_split_before_its_last_ten_characters() {
        assert_eq!(middle("order-17").into_string(), "order-17");
        let edge = "a".repeat(MIDDLE_FROM);
        assert_eq!(middle(&edge).into_string(), edge, "28 is not yet long");
        let long = "orders-2026-11-12-checkout-ab3f9c01d2";
        assert_eq!(
            middle(long).into_string(),
            format!(
                r#"<span class="mid" title="{long}"><span class="mid-a">orders-2026-11-12-checkout-</span><span class="mid-b">ab3f9c01d2</span></span>"#
            )
        );
    }

    #[test]
    fn the_two_halves_are_the_whole_text_and_never_cut_a_character() {
        for text in ["é".repeat(40), "日本語".repeat(20), "𝔘".repeat(33)] {
            let m = middle(&text).into_string();
            let a = m
                .split("mid-a\">")
                .nth(1)
                .unwrap()
                .split("</span>")
                .next()
                .unwrap();
            let b = m
                .split("mid-b\">")
                .nth(1)
                .unwrap()
                .split("</span>")
                .next()
                .unwrap();
            assert_eq!(format!("{a}{b}"), text);
            assert_eq!(b.chars().count(), TAIL);
        }
    }

    #[test]
    fn a_hostile_text_is_escaped_in_the_title_and_in_both_halves() {
        let hostile = "<img src=x onerror=alert(1)>\"'</span><script>alert(1)</script>";
        let m = middle(hostile).into_string();
        assert!(!m.contains("<img") && !m.contains("<script"), "{m}");
        assert!(
            m.contains("&lt;img src=x") && m.contains("title=\"&lt;img"),
            "{m}"
        );
    }
}
