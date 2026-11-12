//! The DataTable: the one table of the console's data pages (CON-808, `docs/port/console/live-design.md`).
//!
//! An automatic table lays its columns out from the rows it holds, so every time rows of other widths arrive the columns compress and
//! spread. This one never does: `table-layout: fixed` with a `<colgroup>` the PAGE declares (a width in pixels or percent for every
//! column but one, which fills what is left), cells that do not wrap and give way to an ellipsis (the full value in `title`),
//! numbers right-aligned in tabular figures, a row height fixed by the density (comfortable 44 px, compact 36 px), every row
//! with a stable id, and a header that is never part of a refresh (`data-morph-skip`; the tail only ever touches `tbody`). The
//! empty state has a reserved height. Nothing in the layout depends on the rows held.

use maud::{Markup, html};

/// A column's width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    /// Fixed pixels.
    Px(u16),
    /// A share of the table.
    Pct(u8),
    /// What is left: exactly one column of a table should be this.
    Fill,
}

/// The row height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Density {
    /// 44 px.
    Comfortable,
    /// 36 px.
    Compact,
    /// 56 px: two lines in a cell (a name and what is said under it).
    Roomy,
}

impl Density {
    /// The row height in pixels.
    #[must_use]
    pub const fn px(self) -> u16 {
        match self {
            Self::Comfortable => 44,
            Self::Compact => 36,
            Self::Roomy => 56,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Comfortable => "comfortable",
            Self::Compact => "compact",
            Self::Roomy => "roomy",
        }
    }
}

/// A column.
#[derive(Debug, Clone, Copy)]
pub struct Col<'a> {
    pub name: &'a str,
    pub width: Width,
    /// Numbers: right-aligned, tabular figures.
    pub right: bool,
    /// Long text: the cell may wrap (the row then has no fixed height; a table that windows has none of these).
    pub wrap: bool,
}

impl<'a> Col<'a> {
    #[must_use]
    pub const fn new(name: &'a str, width: Width) -> Self {
        Self {
            name,
            width,
            right: false,
            wrap: false,
        }
    }

    #[must_use]
    pub const fn right(mut self) -> Self {
        self.right = true;
        self
    }

    #[must_use]
    pub const fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }
}

/// One row: a stable key (a request id, a topic, a group) and a cell per column.
#[derive(Debug)]
pub struct Row {
    pub key: String,
    pub cells: Vec<Markup>,
    pub class: Option<&'static str>,
}

impl Row {
    #[must_use]
    pub fn new(key: impl Into<String>, cells: Vec<Markup>) -> Self {
        Self {
            key: key.into(),
            cells,
            class: None,
        }
    }

    #[must_use]
    pub const fn class(mut self, class: &'static str) -> Self {
        self.class = Some(class);
        self
    }
}

/// A table's declaration.
#[derive(Debug, Clone, Copy)]
pub struct DataTable<'a> {
    /// The table's id (unique on the page); its rows are `{id}-r-{key}`.
    pub id: &'a str,
    /// The table's accessible name.
    pub label: &'a str,
    pub cols: &'a [Col<'a>],
    pub density: Density,
    /// A tail: `tbody` takes rows at the top from the script (`data-tail`).
    pub tail: bool,
    /// What the table says when it has no rows, in a block of reserved height.
    pub empty: &'a str,
}

impl<'a> DataTable<'a> {
    #[must_use]
    pub const fn new(id: &'a str, label: &'a str, cols: &'a [Col<'a>]) -> Self {
        Self {
            id,
            label,
            cols,
            density: Density::Comfortable,
            tail: false,
            empty: "Nothing to show.",
        }
    }

    #[must_use]
    pub const fn compact(mut self) -> Self {
        self.density = Density::Compact;
        self
    }

    #[must_use]
    pub const fn roomy(mut self) -> Self {
        self.density = Density::Roomy;
        self
    }

    #[must_use]
    pub const fn tail(mut self) -> Self {
        self.tail = true;
        self
    }

    #[must_use]
    pub const fn empty(mut self, text: &'a str) -> Self {
        self.empty = text;
        self
    }
}

/// The id of a row of the table `table`.
#[must_use]
pub fn row_id(table: &str, key: &str) -> String {
    let safe: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{table}-r-{safe}")
}

fn col(c: &Col<'_>) -> Markup {
    match c.width {
        Width::Px(p) => html! { col style={"width:" (p) "px"}; },
        Width::Pct(p) => html! { col style={"width:" (p) "%"}; },
        Width::Fill => html! { col; },
    }
}

/// A cell of plain text: it gives way to an ellipsis and keeps its full value for the hover.
#[must_use]
pub fn text(value: &str) -> Markup {
    html! { span class="ell" title=(value) { (value) } }
}

/// The table.
#[must_use]
pub fn data_table(t: &DataTable<'_>, rows: Vec<Row>) -> Markup {
    let empty = rows.is_empty();
    html! {
        div class={"dt " (t.density.name())} data-density=(t.density.name()) {
            table class="t dt-t" id=(t.id) aria-label=(t.label) {
                colgroup { @for c in t.cols { (col(c)) } }
                thead data-morph-skip { tr { @for c in t.cols { th scope="col" class=[c.right.then_some("r")] { (c.name) } } } }
                tbody data-tail=[t.tail.then_some("")] { @for r in &rows { (row_markup(t, r)) } }
            }
            div class="dt-empty" data-empty hidden[!empty] { (t.empty) }
        }
    }
}

/// One row, as the table draws it: the tail sends rows in this form to be put at the top of a table's body.
#[must_use]
pub fn row_markup(t: &DataTable<'_>, r: &Row) -> Markup {
    html! {
        tr id=(row_id(t.id, &r.key)) data-row=(r.key) class=[r.class] {
            @for (i, cell) in r.cells.iter().enumerate() {
                @let c = t.cols.get(i);
                td class=[cell_class(c)] { (cell) }
            }
        }
    }
}

fn cell_class(c: Option<&Col<'_>>) -> Option<&'static str> {
    match c {
        Some(c) if c.right && c.wrap => Some("r wrap"),
        Some(c) if c.right => Some("r"),
        Some(c) if c.wrap => Some("wrap"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLS: [Col<'static>; 3] = [
        Col::new("Time", Width::Px(120)),
        Col::new("Target", Width::Fill),
        Col::new("Took", Width::Pct(10)).right(),
    ];

    #[test]
    fn the_page_declares_the_widths_and_the_table_never_takes_them_from_its_rows() {
        let m = data_table(
            &DataTable::new("tape", "The wire", &COLS).compact(),
            vec![Row::new("7", vec![text("a"), text("b"), text("1 ms")])],
        )
        .into_string();
        assert!(
            m.contains(
                r#"<colgroup><col style="width:120px"><col><col style="width:10%"></colgroup>"#
            ),
            "{m}"
        );
        assert!(m.contains(r#"<thead data-morph-skip>"#));
        assert!(m.contains(r#"<tr id="tape-r-7" data-row="7">"#));
        assert!(m.contains(r#"<td class="r"><span class="ell" title="1 ms">1 ms</span></td>"#));
        assert!(m.contains(r#"data-density="compact""#));
        assert!(m.contains(r#"data-empty hidden"#));
    }

    #[test]
    fn an_empty_table_keeps_its_header_and_says_so_in_a_reserved_block() {
        let m = data_table(
            &DataTable::new("t", "T", &COLS).empty("No requests yet."),
            vec![],
        )
        .into_string();
        assert!(m.contains("<th scope=\"col\">Time</th>"));
        assert!(
            m.contains(r#"<div class="dt-empty" data-empty>No requests yet.</div>"#),
            "{m}"
        );
    }

    #[test]
    fn a_row_id_is_safe_for_any_key() {
        assert_eq!(row_id("t", "a b/\"c"), "t-r-a_b__c");
        assert_eq!(row_id("t", "orders-1"), "t-r-orders-1");
    }

    #[test]
    fn a_tail_table_says_so_on_its_body() {
        let m = data_table(&DataTable::new("t", "T", &COLS).tail(), vec![]).into_string();
        assert!(m.contains("<tbody data-tail"), "{m}");
    }
}
