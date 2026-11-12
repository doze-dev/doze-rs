//! The topics list as a [`DataTable`]: the columns are declared here, one row per topic with its name as the row's key.

use maud::{Markup, html};

use super::common::policy_word;
use crate::base::{Base, segment};
use crate::format::bytes;
use crate::model::{Cleanup, TopicRow, TopicsView};
use crate::ui::button::{State, state};
use crate::ui::datatable::{Col, DataTable, Row, Width, data_table};
use crate::ui::metrics::at_cell;
use crate::ui::table::middle;
use crate::words::retention;

const COLS: [Col<'static>; 5] = [
    Col::new("Name", Width::Fill),
    Col::new("Partitions", Width::Px(104)).right(),
    Col::new("Cleanup policy", Width::Px(150)),
    Col::new("Size on disk", Width::Px(120)).right(),
    Col::new("Last write", Width::Px(120)).right(),
];

/// The table's id.
pub const TOPICS_TABLE: &str = "topics";

fn cleanup_tag(c: Cleanup) -> Markup {
    html! { span class="tag" { (policy_word(c)) } }
}

fn sub_line(r: &TopicRow) -> Option<String> {
    if r.internal {
        return Some("read only · the group coordinator's name for where commits live".to_owned());
    }
    if r.empty {
        return Some("empty: no records yet".to_owned());
    }
    let mut parts = Vec::new();
    if let Some(n) = r.segments.filter(|n| *n > 1) {
        parts.push(format!("{n} segments"));
    }
    if let Some(ms) = r.retention_ms {
        parts.push(format!("retention {}", retention(ms)));
    }
    if !r.read_by.is_empty() {
        parts.push(format!("read by {}", r.read_by.join(", ")));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn row(r: &TopicRow, now_ms: i64, base: &Base) -> Row {
    let href = base.url(&format!("/topics/{}", segment(&r.name)));
    let sub = sub_line(r);
    let name = html! {
        @if r.internal {
            span class="mono" { (r.name) } " " span class="tag" { "internal" }
        } @else {
            a class="mono name-link" href=(href) { (middle(&r.name)) }
            @if r.offline > 0 { " " (state(State::Bad, "Offline")) }
            @if r.txn_open { " " (state(State::Warn, "Transaction open")) }
        }
        @if let Some(s) = &sub { span class="sub ell" title=(s) { (s) } }
    };
    let (size, last) = if r.internal {
        (
            html! { span class="mono dim" { "not a log" } },
            html! { span class="mono dim" { "none" } },
        )
    } else {
        (
            html! { span class="mono" { (r.bytes.map_or_else(|| "unknown".to_owned(), bytes)) } },
            match r.last_write_ms {
                Some(ms) if !r.empty => at_cell(ms, now_ms),
                _ => html! { span class="mono" { "never" } },
            },
        )
    };
    Row::new(
        r.name.clone(),
        vec![
            name,
            html! { span class="mono" { (r.partitions) } },
            cleanup_tag(r.cleanup),
            size,
            last,
        ],
    )
}

/// The table of the list.
#[must_use]
pub(super) fn table(v: &TopicsView, base: &Base) -> Markup {
    data_table(
        &DataTable::new(TOPICS_TABLE, "Topics", &COLS)
            .roomy()
            .empty("No topic matches."),
        v.rows.iter().map(|r| row(r, v.now_ms, base)).collect(),
    )
}

/// The table of one row the note under the list shows: what the internal topic would look like.
#[must_use]
pub(super) fn internal_table(r: &TopicRow, base: &Base) -> Markup {
    data_table(
        &DataTable::new("topics-internal", "The internal topic", &COLS).roomy(),
        vec![row(r, 0, base)],
    )
}
