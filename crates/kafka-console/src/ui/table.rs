//! Tables: a header of column names and rows of cells that are markup (so a cell can hold a link or a state word).

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
}
