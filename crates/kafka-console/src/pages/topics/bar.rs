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
use crate::ui::components::{Choice, SegItem, Select, seg, select};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::inplace::{self, Opts};
use crate::ui::kafka::filter_box;

/// The id of the toolbar: the controls a script reads the whole form of. The data it drives is `#topic-messages`.
pub(super) const BAR_ID: &str = "topic-bar";

/// The most partitions drawn as a row of segments; a topic with more has a Select (a list you can open, with the keys).
const MOST_SEGMENTS: usize = 8;

/// The partitions: a Segmented control for a few, a Select for many. Each choice is the same page with that partition, so a
/// choice is also a plain link (the Select is a real `<select>` that submits the form with no script).
fn partition_pick(v: &MessagesView, base: &Base) -> Markup {
    let to = |p: Option<i32>| {
        let mut q = v.query.clone();
        q.partition = p;
        messages_url(base, &v.head.name, &q)
    };
    if v.head.parts.len() > MOST_SEGMENTS {
        let labels: Vec<String> = v
            .head
            .parts
            .iter()
            .map(|p| p.partition.to_string())
            .collect();
        let mut choices = vec![Choice::new("", "All partitions")];
        choices.extend(labels.iter().map(|l| Choice::new(l, l)));
        let now = v.query.partition.map(|p| p.to_string()).unwrap_or_default();
        return select(&Select {
            auto_submit: true,
            placeholder: "All partitions",
            ..Select::new("topic-partition", "partition", "Partition", &choices, &now)
        });
    }
    let links: Vec<String> = std::iter::once(to(None))
        .chain(v.head.parts.iter().map(|p| to(Some(p.partition))))
        .collect();
    let labels: Vec<String> = std::iter::once("All partitions".to_owned())
        .chain(v.head.parts.iter().map(|p| p.partition.to_string()))
        .collect();
    let items: Vec<SegItem<'_>> = labels
        .iter()
        .zip(&links)
        .enumerate()
        .map(|(i, (label, href))| SegItem {
            label,
            href,
            current: if i == 0 {
                v.query.partition.is_none()
            } else {
                v.query.partition == Some(v.head.parts[i - 1].partition)
            },
            count: None,
        })
        .collect();
    seg("Partition", &items, Some(TOPIC_VIEW))
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
        partition: Some(p.partition),
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
            label class="bar-time" { span class="sr" { "From time (UTC)" } input type="text" name="time" value=(q.time.as_ref().map_or("", |t| t.1.as_str())) placeholder="From time, UTC" size="16"; }
            (from_held(v, base))
            @if q.filtered() {
                @if let Some(p) = q.partition { (chip(v, base, &format!("Partition {p}"), &without(|c| c.partition = None))) }
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
            @if let Some(p) = q.partition { input type="hidden" name="partition" value=(p); }
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
