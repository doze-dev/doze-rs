//! Delete topic: the dialog (board `topic-delete`) and the page after it (board `topic-deleted`).

use maud::{Markup, html};

use super::{field, submit, topic_url};
use crate::base::Base;
use crate::format::{number, span};
use crate::model::{DeleteView, Deleted, InUse, Refusal, Remaining};
use crate::ui::dialog::{cancel, modal};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::notice::{Tone, callout};
use crate::ui::write::{apicall, field_error, refused, text_input};

fn in_use(u: &InUse) -> Markup {
    let groups: Vec<_> = u.groups.iter().take(3).collect();
    html! {
        b { "It is in use." }
        @for (i, (g, _, members)) in groups.iter().enumerate() {
            @if i == 0 { " Group " } @else { ", group " }
            span class="mono" { (g) }
            " has offsets committed on it (" (members) @if *members == 1 { " member" } @else { " members" } " now)"
        }
        @if u.groups.len() > 3 { ", and " (u.groups.len() - 3) " more groups" }
        @if !u.groups.is_empty() && !u.transactions.is_empty() { ", and" }
        @for (i, t) in u.transactions.iter().take(3).enumerate() {
            @if i == 0 { " transaction " } @else { ", transaction " }
            span class="mono" { (t) }
            " is open on it"
        }
        ". Their next requests are answered UNKNOWN_TOPIC_OR_PARTITION. Commits stay behind: if the topic is made again they will sit past its end."
    }
}

fn body(v: &DeleteView) -> Markup {
    let used = !v.in_use.groups.is_empty() || !v.in_use.transactions.is_empty();
    html! {
        p style="margin:0 0 16px" {
            "This removes " b { (number(v.records)) " records" } " in " (v.partitions) " partitions from disk. It cannot be undone, and the name becomes free for a new, empty topic with a new id."
        }
        @if used { div style="margin-bottom:16px" { (callout(Tone::Warn, Glyph::Warn, in_use(&v.in_use))) } }
        div style="margin-bottom:16px" {
            (field("confirm", html! { "Type the name to confirm" },
                text_input("confirm", "confirm", &v.typed, true, None, v.error.is_some()),
                html! { @if let Some(e) = &v.error { (field_error(e)) } }))
        }
        (apicall("The call", &v.call, None))
    }
}

/// The dialog. `refusal` is the broker's answer when it said no.
#[must_use]
pub fn delete_dialog(
    v: &DeleteView,
    refusal: Option<&Refusal>,
    base: &Base,
    inline: bool,
) -> Markup {
    modal(
        "delete-topic",
        &topic_url(base, &v.topic, "/delete"),
        inline,
        html! { "Delete " span class="mono" { (v.topic) } "?" },
        html! { (body(v)) @if let Some(r) = refusal { div style="margin-top:16px" { (refused(r)) } } },
        html! {
            (cancel(&topic_url(base, &v.topic, "")))
            (submit("btn danger fill", Some("DeleteTopics"), None, html! { "Delete topic" }))
        },
    )
}

fn age(now: i64, then: Option<i64>) -> String {
    match then {
        Some(t) => format!("{} ago", span(((now - t) / 1000).max(0))),
        None => "never".to_owned(),
    }
}

fn row(t: &Remaining, now: i64, base: &Base) -> Markup {
    let href = if t.records == 0 {
        topic_url(base, &t.name, "/messages")
    } else {
        topic_url(base, &t.name, "")
    };
    html! {
        tr {
            td class="name" {
                a class="mono" href=(href) { (t.name) }
                span class="sub" { @if t.records == 0 { "empty: no records yet" } @else { (number(t.records)) " records" } }
            }
            td class="mono r" { (t.partitions) }
            td { span class="tag" { (t.cleanup) } }
            td class="mono r dim" title="The size on disk is read by the topics list" { "—" }
            td class="mono r" data-vary { (age(now, t.last_write_ms)) }
        }
    }
}

/// The page: what was deleted, what happens next, and the topics that are left.
#[must_use]
pub fn deleted_page(d: &Deleted, base: &Base) -> Markup {
    html! {
        div class="banner" style="margin-top:24px" {
            (glyph(Glyph::Check, 18, ""))
            span { b { "Deleted " (d.name) "." } " " (number(d.records)) " records and " (d.partitions) " partitions are gone from disk. The delete is durable: it stays after a power cut." }
        }
        div class="page-h" {
            div { h1 { "Topics" } p class="sub" { (d.topics) @if d.topics == 1 { " topic, " } @else { " topics, " } (d.partitions_left) @if d.partitions_left == 1 { " partition." } @else { " partitions." } } }
            div class="acts" { a class="btn pri" href=(base.url("/topics/new")) { (glyph(Glyph::Plus, 16, "")) "Create topic" } }
        }
        div style="margin-bottom:24px" {
            (callout(Tone::Plain, Glyph::Info, html! {
                b { "What happens next." }
                @if !d.in_use.groups.is_empty() || !d.in_use.transactions.is_empty() { " What read or wrote the topic gets UNKNOWN_TOPIC_OR_PARTITION on its next request." }
                " A topic named " span class="mono" { (d.name) } " made later is a new topic: it starts at offset 0 and never adopts the old records. "
                a class="lnk" href=(base.url("/topics/new")) { "Create it again" }
            }))
        }
        table class="t" {
            thead { tr {
                th scope="col" { "Name" } th scope="col" class="r" { "Partitions" } th scope="col" { "Cleanup policy" }
                th scope="col" class="r" { "Size on disk" } th scope="col" class="r" { "Last write" }
            } }
            tbody { @for t in &d.remaining { (row(t, d.now_ms, base)) } }
        }
        @if d.topics > d.remaining.len() { p class="help" { "Showing " (d.remaining.len()) " of " (d.topics) " topics." } }
    }
}
