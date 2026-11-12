use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::format::{bytes, clock_ms, number};
use crate::model::{Kind, RecordRow, Shown, Standing};
use crate::ui::button::{State, state};
use crate::ui::datatable::{Col, DataTable, Row, Width, data_table, row_markup};
use crate::ui::table::middle;

fn cell(s: &Shown, what: &str) -> Markup {
    html! {
        @match s.kind {
            Kind::Null => span class="dim" { "null" @if what == "value" { " (deletion)" } },
            Kind::Empty => span class="dim" { "empty" },
            _ => (s.preview),
        }
    }
}

/// A key can be long (a UUID with a prefix, a path): its middle gives way, its end stays, the hover has all of it.
fn key_cell(s: &Shown) -> Markup {
    match s.kind {
        Kind::Null | Kind::Empty => cell(s, "key"),
        _ => middle(&s.preview),
    }
}

fn standing(s: Standing) -> Markup {
    match s {
        Standing::Committed => state(State::Ok, "Committed"),
        Standing::AboveLso => state(State::Warn, "Above LSO"),
        Standing::Aborted => state(State::Idle, "Aborted"),
    }
}

const COLS: [Col<'static>; 7] = [
    Col::new("Partition", Width::Px(96)),
    Col::new("Offset", Width::Px(84)),
    Col::new("Timestamp", Width::Px(128)),
    Col::new("Key", Width::Pct(16)),
    Col::new("Value", Width::Fill),
    Col::new("Value size", Width::Px(96)).right(),
    Col::new("For read_committed", Width::Px(150)),
];

/// The table's id; its rows are `records-r-<partition>-<offset>`.
pub(super) const RECORDS: &str = "records";

fn spec(tail: bool) -> DataTable<'static> {
    let t = DataTable::new(RECORDS, "Records, newest first", &COLS)
        .compact()
        .empty("No records to list.");
    if tail { t.tail() } else { t }
}

fn row(r: &RecordRow, topic: &str, query: &str, base: &Base) -> Row {
    // The drawer's address carries the page's own query, so closing the drawer returns to the same filtered page.
    let href = base.url(&format!(
        "/topics/{}/messages/{}/{}{query}",
        segment(topic),
        r.partition,
        r.offset
    ));
    let tone = if r.value.kind == Kind::Binary {
        "color:var(--dim)"
    } else {
        "color:var(--text)"
    };
    let value = html! { span class="ell mono" style={"font-size:12.5px;" (tone)} title=(r.value.preview) { (cell(&r.value, "value")) } };
    Row::new(
        format!("{}-{}", r.partition, r.offset),
        vec![
            html! { span class="mono" { (r.partition) } },
            html! { a class="lnk mono" href=(href) data-dialog { (number(r.offset)) } },
            html! { span class="mono dim" { (clock_ms(r.timestamp_ms)) } },
            html! { span class="mono key" { (key_cell(&r.key)) } },
            value,
            html! { span class="mono" { (bytes(r.value.bytes)) } },
            standing(r.standing),
        ],
    )
}

/// The table: one row per record, newest first. `query` is the page's query (with its `?`, or nothing): a record opens in a
/// drawer over this same page. `tail`: the body takes new rows at the top from the Live tail.
#[must_use]
pub(super) fn records(
    rows: &[RecordRow],
    topic: &str,
    query: &str,
    base: &Base,
    tail: bool,
) -> Markup {
    data_table(
        &spec(tail),
        rows.iter().map(|r| row(r, topic, query, base)).collect(),
    )
}

/// The rows of a tail's event, in the form the table draws them.
#[must_use]
pub fn tail_rows(rows: &[RecordRow], topic: &str, query: &str, base: &Base) -> Markup {
    let t = spec(true);
    html! { @for r in rows { (row_markup(&t, &row(r, topic, query, base))) } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Shown;

    fn shown(kind: Kind, preview: &str) -> Shown {
        Shown {
            kind,
            bytes: 9,
            text: String::new(),
            clipped: false,
            preview: preview.to_owned(),
        }
    }

    #[test]
    fn a_hostile_key_and_value_are_text_in_their_cells_and_the_offset_is_the_link() {
        let hostile = "</script><img src=x onerror=alert(1)>\"'";
        let r = RecordRow {
            partition: 1,
            offset: 16_050,
            timestamp_ms: 0,
            key: shown(Kind::Text, hostile),
            value: shown(Kind::Text, hostile),
            standing: Standing::AboveLso,
        };
        let m = records(&[r], "a/b", "", &Base::root(), false).into_string();
        assert!(!m.contains("<img") && !m.contains("</script>"), "{m}");
        assert!(
            m.contains("&lt;/script&gt;&lt;img src=x onerror=alert(1)&gt;&quot;'"),
            "{m}"
        );
        assert!(
            m.contains(r#"href="/topics/a%2Fb/messages/1/16050""#),
            "{m}"
        );
        assert!(m.contains(r#"<span class="st warn">Above LSO</span>"#));
    }

    #[test]
    fn null_empty_and_binary_have_their_own_words() {
        let r = |k, v| RecordRow {
            partition: 0,
            offset: 1,
            timestamp_ms: 0,
            key: shown(k, ""),
            value: shown(v, "binary · 0a"),
            standing: Standing::Committed,
        };
        let m = records(
            &[r(Kind::Null, Kind::Null), r(Kind::Empty, Kind::Binary)],
            "t",
            "",
            &Base::root(),
            false,
        )
        .into_string();
        assert!(
            m.contains(r#"<span class="dim">null</span>"#)
                && m.contains("null</span>")
                && m.contains("(deletion)")
        );
        assert!(
            m.contains(r#"<span class="dim">empty</span>"#)
                && m.contains("color:var(--dim)\" title=\"binary · 0a\">binary · 0a"),
            "{m}"
        );
    }
}
