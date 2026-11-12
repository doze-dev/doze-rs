//! The table of requests, in the four layouts the boards draw: Live, a topic's dock, a group's wire, and one client's first
//! requests. One row function; the layout says which columns and which words.

use maud::{Markup, html};

use super::parts::{filter_link, request_url, request_url_in, result_cell};
use crate::base::Base;
use crate::format::{clock, date_time, millis};
use crate::model::{WireClass, WireQuery, WireRow};
use crate::ui::button::pill_mono;

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

impl Layout {
    fn columns(self) -> Vec<(&'static str, Option<&'static str>)> {
        let wide = Some("width:142px");
        match self {
            Self::Live => vec![
                ("Time", wide),
                ("Client", Some("width:190px")),
                ("API", Some("width:200px")),
                ("Target", None),
                ("Result", Some("width:270px")),
                ("Took", Some("width:130px")),
            ],
            Self::Topic => [
                "Time",
                "Client",
                "API",
                "Topic and partition",
                "Result",
                "Took",
            ]
            .map(|n| (n, None))
            .to_vec(),
            Self::Group => [
                "Time",
                "Client",
                "Request",
                "What it touched",
                "Result",
                "Took",
            ]
            .map(|n| (n, None))
            .to_vec(),
            Self::First => vec![
                ("Time", Some("width:130px")),
                ("API", Some("width:170px")),
                ("Target", None),
                ("Result", Some("width:330px")),
                ("Took", Some("width:90px")),
            ],
        }
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
    let text = millis(u64::from(r.took_us / 1000));
    let text = if r.took_us < 1000 {
        "<1 ms".to_owned()
    } else {
        text
    };
    html! {
        td class="mono r" data-vary style="white-space:nowrap" { (text) @if let Some(s) = &r.took_sub { span class="sub" { (s) } } }
    }
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
        td class="tm" data-vary {
            a class="lnk" href=(href) data-dialog[on_live] {
                time datetime=(date_time(r.at_ms)) title=(date_time(r.at_ms)) { (crate::format::clock_ms(r.at_ms)) }
            }
            @if let Some(since) = r.since_ms { span class="sub" { "since " (clock(since)) } }
        }
    }
}

fn api(r: &WireRow, base: &Base, q: &WireQuery, filterable: bool) -> Markup {
    let name = html! { b class="mono" { (r.api) } };
    html! {
        td {
            @if filterable { (filter_link(base, q, "api", r.api, name)) } @else { (name) }
            " " span class="dim mono" { "v" (r.version) }
            @if r.transactional { " " span class="tag" { "txn" } }
            @if r.count > 1 { " " span class="fold" { "× " (r.count) } }
        }
    }
}

fn client(r: &WireRow, base: &Base, q: &WireQuery) -> Markup {
    html! {
        td {
            (filter_link(base, q, "client", &r.client, html! { span class="mono" { (r.client) } }))
            @if let Some(t) = &r.tag { " " (pill_mono(t)) }
        }
    }
}

fn target(r: &WireRow, base: &Base, q: &WireQuery, filterable: bool) -> Markup {
    let name = html! { (r.target) };
    html! {
        td class="mono" style="white-space:nowrap" {
            @match (r.topics.first(), filterable) {
                (Some(t), true) => (filter_link(base, q, "topic", t, name)),
                _ => (name),
            }
            @if let Some(s) = &r.target_sub { span class="sub" { (s) } }
        }
    }
}

/// One row.
#[must_use]
pub(super) fn row(r: &WireRow, base: &Base, q: &WireQuery, layout: Layout) -> Markup {
    let filterable = layout == Layout::Live;
    html! {
        tr data-row=(r.id) {
            (time(r, base, q, layout))
            @if layout != Layout::First { (client(r, base, q)) }
            (api(r, base, q, filterable))
            (target(r, base, q, filterable))
            td { (result_cell(r, layout.word_class(r))) }
            (took(r))
        }
    }
}

/// The table: the columns of the layout and one row per request. With no rows, the header alone.
#[must_use]
pub fn table(rows: &[WireRow], base: &Base, q: &WireQuery, layout: Layout) -> Markup {
    html! {
        table class="t wire" {
            thead { tr {
                @for (name, style) in layout.columns() {
                    th scope="col" style=[style] class=[(name == "Took").then_some("r")] { (name) }
                }
            } }
            tbody { @for r in rows { (row(r, base, q, layout)) } }
        }
    }
}
