//! The table of requests, in the four layouts the boards draw: Live, a topic's dock, a group's wire, and one client's first
//! requests. One row function; the layout says which columns and which words. Every layout is a [`DataTable`]: fixed columns
//! the page declares, one line a row, the full text on hover, a stable id on every row (`tape-r-<request id>`), so that a row
//! that arrives, or grows by a folded repeat, changes nothing else.

use maud::{Markup, html};

use super::parts::{filter_link, request_url, request_url_in, result_cell};
use crate::base::Base;
use crate::format::{clock, date_time};
use crate::model::{WireClass, WireQuery, WireRow};
use crate::ui::button::pill_mono;
use crate::ui::datatable::{Col, DataTable, Row, Width, data_table, row_markup};

/// Which board's table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Live: Time, Client, API, Target, Result, Took.
    Live,
    /// A topic's dock: the same columns, the target named "Topic and partition".
    Topic,
    /// A group's wire: "Request" and "What it touched".
    Group,
    /// One client's first requests: no client column.
    First,
}

const LIVE: [Col<'static>; 6] = [
    Col::new("Time", Width::Px(122)),
    Col::new("Client", Width::Px(132)),
    Col::new("API", Width::Px(168)),
    Col::new("Target", Width::Fill),
    Col::new("Result", Width::Px(236)),
    Col::new("Took", Width::Px(84)).right(),
];
const TOPIC: [Col<'static>; 6] = [
    Col::new("Time", Width::Px(122)),
    Col::new("Client", Width::Px(132)),
    Col::new("API", Width::Px(168)),
    Col::new("Topic and partition", Width::Fill),
    Col::new("Result", Width::Px(236)),
    Col::new("Took", Width::Px(84)).right(),
];
const GROUP: [Col<'static>; 6] = [
    Col::new("Time", Width::Px(122)),
    Col::new("Client", Width::Px(132)),
    Col::new("Request", Width::Px(168)),
    Col::new("What it touched", Width::Fill),
    Col::new("Result", Width::Px(236)),
    Col::new("Took", Width::Px(84)).right(),
];
const FIRST: [Col<'static>; 5] = [
    Col::new("Time", Width::Px(122)),
    Col::new("API", Width::Px(168)),
    Col::new("Target", Width::Fill),
    Col::new("Result", Width::Px(260)),
    Col::new("Took", Width::Px(84)).right(),
];

impl Layout {
    const fn cols(self) -> &'static [Col<'static>] {
        match self {
            Self::Live => &LIVE,
            Self::Topic => &TOPIC,
            Self::Group => &GROUP,
            Self::First => &FIRST,
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::Live => "tape",
            Self::Topic => "dock-tape",
            Self::Group => "gwire-tape",
            Self::First => "first-tape",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Live => "The wire",
            Self::Topic => "Requests that touched this topic",
            Self::Group => "Requests that touched this group",
            Self::First => "A client's first requests",
        }
    }

    const fn empty(self) -> &'static str {
        match self {
            Self::Live => {
                "No requests yet. The first one is a good one to watch: ApiVersions, then Metadata, then whatever your client does next."
            }
            _ => "No request has touched this yet.",
        }
    }

    fn spec(self) -> DataTable<'static> {
        let t = DataTable::new(self.id(), self.label(), self.cols())
            .compact()
            .empty(self.empty());
        if self == Self::Live { t.tail() } else { t }
    }

    /// The class of the state word: a topic's dock draws `rw` for the answered and `st` for a refusal, as its board does.
    fn word_class(self, r: &WireRow) -> &'static str {
        match (self, r.class) {
            (Self::Topic, WireClass::Done | WireClass::Told) => "rw",
            _ => "st",
        }
    }
}

fn took(r: &WireRow) -> Markup {
    let text = if r.took_us < 1000 {
        "<1 ms".to_owned()
    } else {
        crate::format::millis(u64::from(r.took_us / 1000))
    };
    html! { span class="ell mono" { (text) } }
}

/// The time of a row, which is its link. On the Live page it opens the request's drawer over the page (carrying the page's own
/// filters, so closing returns to the same tape); in a dock or a group's wire it is a plain link to the drawer's address.
fn time(r: &WireRow, base: &Base, q: &WireQuery, layout: Layout) -> Markup {
    let on_live = layout == Layout::Live;
    let href = if on_live {
        request_url_in(base, r.id, q)
    } else {
        request_url(base, r.id)
    };
    html! {
        a class="lnk" href=(href) data-dialog[on_live] {
            time datetime=(date_time(r.at_ms)) title=(date_time(r.at_ms)) { (crate::format::clock_ms(r.at_ms)) }
        }
    }
}

fn api(r: &WireRow, base: &Base, q: &WireQuery, filterable: bool) -> Markup {
    let name = html! { b class="mono" { (r.api) } };
    html! {
        @if filterable { (filter_link(base, q, "api", r.api, name)) } @else { (name) }
        " " span class="dim mono" { "v" (r.version) }
        @if r.transactional { " " span class="tag" { "txn" } }
        @if r.count > 1 { " " span class="fold" { "× " (r.count) } }
    }
}

fn client(r: &WireRow, base: &Base, q: &WireQuery) -> Markup {
    html! {
        (filter_link(base, q, "client", &r.client, html! { span class="mono" { (r.client) } }))
        @if let Some(t) = &r.tag { " " (pill_mono(t)) }
    }
}

fn target(r: &WireRow, base: &Base, q: &WireQuery, filterable: bool) -> Markup {
    let name = html! { span class="mono" { (r.target) } };
    html! {
        @match (r.topics.first(), filterable) {
            (Some(t), true) => (filter_link(base, q, "topic", t, name)),
            _ => (name),
        }
        @if let Some(s) = &r.target_sub { " " span class="sub" { (s) } }
        @if let Some(since) = r.since_ms { " " span class="sub" { "since " (clock(since)) } }
    }
}

fn result(r: &WireRow, layout: Layout) -> Markup {
    html! { span class="ell" title=(format!("{} {}", r.code_name, r.detail)) { (result_cell(r, layout.word_class(r))) } }
}

/// One row of the table: its key is the request's id.
fn row(r: &WireRow, base: &Base, q: &WireQuery, layout: Layout) -> Row {
    let filterable = layout == Layout::Live;
    let mut cells = vec![time(r, base, q, layout)];
    if layout != Layout::First {
        cells.push(client(r, base, q));
    }
    cells.push(api(r, base, q, filterable));
    cells.push(target(r, base, q, filterable));
    cells.push(result(r, layout));
    cells.push(took(r));
    Row::new(r.id.to_string(), cells)
}

/// The table: the columns of the layout and one row per request. With no rows, the header and a block that says why.
#[must_use]
pub fn table(rows: &[WireRow], base: &Base, q: &WireQuery, layout: Layout) -> Markup {
    table_with_empty(rows, base, q, layout, layout.empty())
}

/// [`table`] with its own words for the empty block.
#[must_use]
pub fn table_with_empty(
    rows: &[WireRow],
    base: &Base,
    q: &WireQuery,
    layout: Layout,
    empty: &str,
) -> Markup {
    data_table(
        &layout.spec().empty(empty),
        rows.iter().map(|r| row(r, base, q, layout)).collect(),
    )
}

/// The rows of a tail's event: `<tr>`s of the Live table, newest first, in the form the table draws them.
#[must_use]
pub fn tail_rows(rows: &[WireRow], base: &Base, q: &WireQuery) -> Markup {
    let spec = Layout::Live.spec();
    html! { @for r in rows { (row_markup(&spec, &row(r, base, q, Layout::Live))) } }
}
