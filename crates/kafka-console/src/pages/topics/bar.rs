//! The toolbar of the Messages page: ONE layout that never moves.
//!
//! Row one is always the partitions, the find box and, at the right, "Reading as" with its two levels in a fixed order. Row two
//! is always there and always one line tall: the time box, (the shortcut to a held record when the topic has one) and the
//! filters that are on, each with a way to take it off, or a quiet sentence when none is. Choosing a partition, a filter or an
//! isolation level changes the data under the toolbar and what the second row says, never where anything is.
//!
//! It is a plain GET form: the partition and the level are submit buttons (so it works with no script, and a script can send
//! the same form and swap `#topic-messages`), and the state the buttons do not carry travels in hidden fields that come
//! AFTER the buttons (a form data set lists the clicked button first, and the page reads the first value of a name).

use maud::{Markup, html};

use super::common::topic_url;
use super::messages::{TOPIC_VIEW, messages_url};
use crate::base::Base;
use crate::format::number;
use crate::model::{MessageQuery, MessagesView, Reading};
use crate::ui::components::{Choice, MultiSelect, SegItem, multiselect, popover, seg};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::inplace::{self, Opts};
use crate::ui::kafka::filter_box;

/// The id of the toolbar: the controls a script reads the whole form of. The data it drives is `#topic-messages`.
pub(super) const BAR_ID: &str = "topic-bar";

/// The most partitions drawn as a row of segments; a topic with more has a Select (a list you can open, with the keys).
const MOST_SEGMENTS: usize = 8;

/// The partitions. A topic with few has a Segmented control (one partition or all: each choice is the same page, a plain link) and, after
/// it, "More", a MultiSelect for several at once; a topic with many has the MultiSelect alone ("All partitions"). The MultiSelect is a
/// group of checkboxes in a form with no script and sends `partition=0&partition=2`.
fn partition_pick(v: &MessagesView, base: &Base) -> Markup {
    let labels: Vec<String> = v
        .head
        .parts
        .iter()
        .map(|p| p.partition.to_string())
        .collect();
    let choices: Vec<Choice<'_>> = labels.iter().map(|l| Choice::new(l, l)).collect();
    let chosen: Vec<String> = v.query.partitions.iter().map(ToString::to_string).collect();
    let many = v.head.parts.len() > MOST_SEGMENTS;
    let multi = multiselect(&MultiSelect {
        auto_submit: true,
        placeholder: if many { "All partitions" } else { "More" },
        noun: "partitions",
        class: "dz-chip",
        ..MultiSelect::new(
            "topic-partition",
            "partition",
            "Partitions",
            &choices,
            &chosen,
        )
    });
    if many {
        return multi;
    }
    let to = |p: Option<i32>| {
        let mut q = v.query.clone();
        q.partitions = p.into_iter().collect();
        messages_url(base, &v.head.name, &q)
    };
    // Each segment: its label, the page with that partition, and whether it is the only one in the query.
    let mut rows: Vec<(String, String, bool)> = vec![(
        "All partitions".to_owned(),
        to(None),
        v.query.partitions.is_empty(),
    )];
    rows.extend(v.head.parts.iter().map(|p| {
        (
            p.partition.to_string(),
            to(Some(p.partition)),
            v.query.partitions == [p.partition],
        )
    }));
    let items: Vec<SegItem<'_>> = rows
        .iter()
        .map(|(label, href, current)| SegItem {
            label,
            href,
            current: *current,
            count: None,
        })
        .collect();
    html! { (seg("Partition", &items, Some(TOPIC_VIEW))) (multi) }
}

/// "Reading as", always `read_uncommitted` then `read_committed`, always at the right.
fn reading_seg(v: &MessagesView, base: &Base) -> Markup {
    let to = |r: Reading| {
        let mut q = v.query.clone();
        q.reading = r;
        messages_url(base, &v.head.name, &q)
    };
    let (u, c) = (to(Reading::Uncommitted), to(Reading::Committed));
    let items = [
        SegItem {
            label: Reading::Uncommitted.word(),
            href: &u,
            current: v.query.reading == Reading::Uncommitted,
            count: None,
        },
        SegItem {
            label: Reading::Committed.word(),
            href: &c,
            current: v.query.reading == Reading::Committed,
            count: None,
        },
    ];
    html! {
        span class="dim" { "Reading as" }
        (seg("Reading as", &items, Some(TOPIC_VIEW)))
    }
}

/// What the find box shows: the key, or the offset. A key that is only digits would read back as an offset, so it is kept
/// in a hidden field instead.
fn find_value(q: &MessageQuery) -> (String, bool) {
    match (&q.key, q.offset) {
        (Some(k), _) if !k.bytes().all(|b| b.is_ascii_digit()) => (k.clone(), true),
        (None, Some(o)) => (o.to_string(), true),
        _ => (String::new(), false),
    }
}

fn from_held(v: &MessagesView, base: &Base) -> Markup {
    let Some(p) = v.head.parts.iter().find(|p| p.held() > 0) else {
        return html! {};
    };
    let q = MessageQuery {
        partitions: vec![p.partition],
        offset: Some(p.last_stable),
        ..MessageQuery::default()
    };
    html! {
        (inplace::link_with(TOPIC_VIEW, "btn sm", &messages_url(base, &v.head.name, &q), &Opts { op: Some("Fetch"), ..Opts::default() }, html! {
            (glyph(Glyph::Filter, 14, "")) "Show " (v.head.name) "-" (p.partition) " from " (number(p.last_stable))
        }))
    }
}

/// A filter that is on: its words and the link that takes it off.
fn chip(v: &MessagesView, base: &Base, label: &str, without: &MessageQuery) -> Markup {
    html! {
        (inplace::link(TOPIC_VIEW, "fchip on rm", &messages_url(base, &v.head.name, without), Some(&format!("Remove the filter {label}")), html! {
            (label) (glyph(Glyph::X, 12, ""))
        }))
    }
}

/// "From time": a Popover with a proper field (what to write, an Apply), in the row's fixed place. The field is part of the form, so
/// Enter or Apply asks for the time, and every other control of the toolbar carries it along.
fn time_pick(q: &MessageQuery) -> Markup {
    let body = html! {
        div class="bar-time" {
            label class="bar-time-l" for="bar-time-in" { "From time (UTC)" }
            input id="bar-time-in" type="text" name="time" value=(q.time.as_ref().map_or("", |t| t.1.as_str())) placeholder="2026-11-09 22:13" size="22" autocomplete="off" spellcheck="false";
            p class="help" style="margin:8px 0 12px" { "Or milliseconds since the epoch. The first record at or after it." }
            button class="btn sm pri" type="submit" { "Apply" }
        }
    };
    popover("topic-time", "From time", &body)
}

/// Row two: the time box, the shortcut to a held record, the filters that are on, or one quiet sentence.
fn chips(v: &MessagesView, base: &Base) -> Markup {
    let q = &v.query;
    let without = |f: fn(&mut MessageQuery)| {
        let mut c = q.clone();
        f(&mut c);
        c
    };
    html! {
        div class="bar-chips" {
            (time_pick(q))
            (from_held(v, base))
            @if q.filtered() {
                @for p in &q.partitions { (chip(v, base, &format!("Partition {p}"), &MessageQuery { partitions: q.partitions.iter().copied().filter(|x| x != p).collect(), ..q.clone() })) }
                @if let Some(o) = q.offset { (chip(v, base, &format!("From offset {}", number(o)), &without(|c| c.offset = None))) }
                @if let Some((_, t)) = &q.time { (chip(v, base, &format!("From {t}"), &without(|c| c.time = None))) }
                @if let Some(k) = &q.key { (chip(v, base, &format!("Key {k}"), &without(|c| c.key = None))) }
                (inplace::link_with(TOPIC_VIEW, "btn sm", &messages_url(base, &v.head.name, &MessageQuery { reading: q.reading, ..MessageQuery::default() }), &Opts { ui: Some("clear"), ..Opts::default() }, html! {
                    (glyph(Glyph::X, 14, "")) "Clear filters"
                }))
            } @else {
                span class="dim" { "No filters: the newest records of every partition." }
            }
        }
    }
}

/// The toolbar.
#[must_use]
pub(super) fn bar(v: &MessagesView, base: &Base) -> Markup {
    let q = &v.query;
    let (find, shown) = find_value(q);
    let action = format!("{}/messages", topic_url(base, &v.head.name));
    html! {
        (inplace::form(TOPIC_VIEW, Some(BAR_ID), "toolbar bar", None, &action, html! {
            // The button Enter presses in the find box: not a control of the page, so it is out of the board's reckoning.
            input type="submit" value="Show" aria-label="Show records" hidden data-scaffold;
            div class="bar-row" {
                (partition_pick(v, base))
                (filter_box("find", "Key, or offset", "Find by key or offset", &find))
                span class="sp" {}
                (reading_seg(v, base))
            }
            // After the buttons, so that the button that was clicked is the first of its name.
            @if q.reading == Reading::Committed { input type="hidden" name="isolation" value="read_committed"; }
            @if !shown {
                @if let Some(k) = &q.key { input type="hidden" name="key" value=(k); }
                @if let Some(o) = q.offset { input type="hidden" name="offset" value=(o); }
            }
            @if shown {
                @if let (Some(o), Some(_)) = (q.offset, &q.key) { input type="hidden" name="offset" value=(o); }
            }
            @if q.limit != crate::model::DEFAULT_LIMIT { input type="hidden" name="limit" value=(q.limit); }
            (chips(v, base))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(key: Option<&str>, offset: Option<i64>) -> MessageQuery {
        MessageQuery {
            key: key.map(str::to_owned),
            offset,
            ..MessageQuery::default()
        }
    }

    #[test]
    fn the_find_box_shows_a_key_or_an_offset_and_never_a_key_that_would_read_back_as_an_offset() {
        assert_eq!(
            find_value(&q(Some("ord-1"), None)),
            ("ord-1".to_owned(), true)
        );
        assert_eq!(
            find_value(&q(None, Some(16_098))),
            ("16098".to_owned(), true)
        );
        assert_eq!(
            find_value(&q(Some("42"), None)),
            (String::new(), false),
            "a key of digits is a hidden field"
        );
        assert_eq!(find_value(&q(Some("42"), Some(7))), (String::new(), false));
        assert_eq!(find_value(&q(None, None)), (String::new(), false));
    }
}
