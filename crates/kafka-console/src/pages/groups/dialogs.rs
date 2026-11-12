//! The three dialogs of a group: Reset offsets (with its preview), Delete group, Delete offsets. Each is an overlay (a dialog of
//! `ui::overlay`) over the group's lag page, or its page for Delete group, with one POST form; the page that follows a refusal
//! is in `refused.rs`.

use maud::{Markup, html};

use super::common::group_url;
use crate::base::Base;
use crate::format::number;
use crate::lag::Lag;
use crate::model::{
    GroupDeleteView, GroupHead, GroupPhase, OffsetDeleteView, Refusal, ResetRow, ResetView,
};
use crate::ui::code::block;
use crate::ui::components::{Choice, Select, select};
use crate::ui::dialog::{cancel, modal};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::notice::{Tone, callout};

fn field(id: &str, label: &str, control: Markup) -> Markup {
    html! { div class="field" { label for=(id) { (label) } (control) } }
}

fn call(lines: &crate::model::Lines) -> Markup {
    html! { div data-extra="api-preview" style="margin-top:16px" { (block(lines, None, Some("font-size:12.5px"))) } }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn members_callout(h: &GroupHead, refused_text: &str) -> Markup {
    if h.members.is_empty() {
        callout(
            Tone::Plain,
            Glyph::Check,
            html! { b { (h.id) " has no members," } " so Kafka accepts a commit with no generation. With a member in the group it would be refused." },
        )
    } else {
        html! {
            div data-extra="teaching" {
                (callout(Tone::Warn, Glyph::Warn, html! {
                    b { "This group is not empty." } " " (members_names(h)) " " (refused_text)
                }))
            }
        }
    }
}

fn members_names(h: &GroupHead) -> String {
    match h.members.as_slice() {
        [one] => format!("{} is a member.", one.id),
        many => format!("{} members are in it.", many.len()),
    }
}

fn lag_after(r: &ResetRow) -> String {
    match r.lag_after {
        Lag::Behind(n) => number(n),
        Lag::CaughtUp => "0".to_owned(),
        Lag::PastEnd(by) => format!("{} past the end", number(by)),
        Lag::NoCommit | Lag::Gone => "—".to_owned(),
    }
}

fn preview(v: &ResetView) -> Markup {
    html! {
        div class="eyebrow" style="margin:20px 0 8px" { "Preview" }
        table class="t" data-extra="teaching" {
            thead { tr {
                th scope="col" { "Partition" } th scope="col" class="r" { "Committed now" }
                th scope="col" class="r" { "After" } th scope="col" class="r" { "Lag after" }
            } }
            tbody {
                @for r in &v.rows {
                    tr {
                        td class="name mono" { (v.topic) "-" (r.partition) }
                        td class="r mono" { @match r.committed { Some(c) => (number(c)), None => span class="dim" { "none" } } }
                        td class="r mono" { @match r.after { Some(a) => (number(a)), None => span class="dim" { "stays" } } }
                        td class="r mono" { (lag_after(r)) }
                    }
                }
            }
        }
    }
}

fn option(
    v: &ResetView,
    value: &str,
    title: &str,
    sub: Option<&str>,
    input: Option<&str>,
) -> Markup {
    let id = format!("reset-to-{value}");
    html! {
        label class="opt-row" for=(id) {
            input type="radio" id=(id) name="to" value=(value) checked[v.to == value];
            span class="radio" {}
            div {
                div class="t1" { (title) }
                @if let Some(s) = sub { div class="t2" { (s) } }
                @if let Some(label) = input {
                    div class="t2 opt-in" { div class="input mono" { input type="text" name="value" aria-label=(label) value=(if v.to == value { v.value.as_str() } else { "" }) autocomplete="off" spellcheck="false"; } }
                }
            }
        }
    }
}

fn topic_field(v: &ResetView) -> Markup {
    let n = v.rows.len();
    let labels: Vec<String> = v
        .topics
        .iter()
        .map(|t| {
            if *t == v.topic {
                format!(
                    "{t}, {}",
                    if n == 1 {
                        "its partition".to_owned()
                    } else {
                        format!("all {n} partitions")
                    }
                )
            } else {
                t.clone()
            }
        })
        .collect();
    let choices: Vec<Choice<'_>> = v
        .topics
        .iter()
        .zip(&labels)
        .map(|(t, l)| Choice::new(t, l))
        .collect();
    field(
        "reset-topic",
        "Topic",
        select(&Select {
            class: "dz-block mono",
            ..Select::new("reset-topic", "topic", "Topic", &choices, &v.topic)
        }),
    )
}

fn body(v: &ResetView) -> Markup {
    let h = &v.head;
    html! {
        p style="margin:0 0 16px" { "Moves where " (h.id) " will read next. Nothing is read or deleted; only the committed offsets change." }
        div style="margin-bottom:20px" { (members_callout(h, "A reset is a commit with no generation, which Kafka accepts only while nobody is in the group, so it will refuse with UNKNOWN_MEMBER_ID. Stop the app first, or try it to see the answer.")) }
        @if v.topics.is_empty() {
            p class="help" { (h.id) " has committed no offset, so there is nothing to move. The first commit from its app makes a row here." }
        } @else {
            (topic_field(v))
            div class="field" style="margin-top:16px" {
                div class="lab" { "Reset to" }
                div class="stack-y" style="gap:10px" {
                    (option(v, "earliest", "Earliest", Some("The log start of each partition, asked of ListOffsets"), None))
                    (option(v, "latest", "Latest", Some("The log end of each partition, asked of ListOffsets"), None))
                    (option(v, "offset", "A specific offset", None, Some("Offset")))
                    (option(v, "time", "A time", None, Some("Time, like 2026-11-06 00:14 UTC")))
                    (option(v, "shift", "Shift by", Some("Back (negative) or forward from where each partition is"), Some("Records")))
                    (option(v, "current", "Where it is now", Some("Each committed offset, kept within its log"), None))
                }
            }
            @if let Some(e) = &v.error { p class="help err" role="alert" style="margin-top:12px" { (e) } }
            (preview(v))
            (call(&v.call))
        }
    }
}

/// The Reset dialog, over the group's Lag page.
#[must_use]
pub fn reset_dialog(v: &ResetView, base: &Base) -> Markup {
    let moves = v.rows.iter().filter(|r| r.after.is_some()).count();
    let h = &v.head;
    modal(
        "reset-offsets",
        &group_url(base, &h.id, "/reset"),
        &group_url(base, &h.id, "/lag"),
        html! { "Reset offsets of " (h.id) },
        body(v),
        html! {
            (cancel(&group_url(base, &h.id, "/lag")))
            button class="btn" type="submit" formmethod="get" formaction=(group_url(base, &h.id, "/reset")) data-ui="refresh" { "Preview" }
            button class="btn pri" type="submit" data-op="OffsetCommit" disabled[moves == 0] { "Reset " (plural(moves, "offset", "offsets")) }
        },
    )
}

/// The Delete-group dialog, over the group's page.
#[must_use]
pub fn delete_dialog(v: &GroupDeleteView, base: &Base) -> Markup {
    let h = &v.head;
    let n: usize = v.offsets.iter().map(|(_, ps)| ps.len()).sum();
    let named: Vec<String> = v
        .offsets
        .iter()
        .map(|(t, ps)| {
            super::detail::part_list(
                &ps.iter()
                    .map(|p| crate::model::GroupPart {
                        topic: t.clone(),
                        partition: *p,
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    modal(
        "delete-group",
        &group_url(base, &h.id, "/delete"),
        &group_url(base, &h.id, ""),
        html! { "Delete " (h.id) "?" },
        html! {
            p style="margin:0 0 16px" {
                "Deleting a group removes the group and its " (plural(n, "committed offset", "committed offsets"))
                @if n > 0 { ": " (named.join(", ")) } ". The app would start again from its "
                span class="mono" { "auto.offset.reset" } "."
            }
            @if !h.members.is_empty() {
                div data-extra="teaching" { (callout(Tone::Warn, Glyph::Warn, html! {
                    b { "This group is not empty." } " " (members_names(h))
                    " DeleteGroups only accepts an empty group, so Kafka will refuse with NON_EMPTY_GROUP. Stop the app first, or try it to see the answer."
                })) }
            }
        },
        html! {
            (cancel(&group_url(base, &h.id, "")))
            button class="btn danger fill" type="submit" data-op="DeleteGroups" { "Delete group" }
        },
    )
}

/// The Delete-offsets dialog, over the Lag page with the expiring offsets; `refusal` is Kafka's answer when it said no.
#[must_use]
pub fn offset_delete_dialog(
    v: &OffsetDeleteView,
    refusal: Option<&Refusal>,
    base: &Base,
) -> Markup {
    let h = &v.head;
    let picked: usize = v
        .topics
        .iter()
        .filter(|(t, _)| v.chosen.contains(t))
        .map(|(_, ps)| ps.len())
        .sum();
    modal(
        "delete-offsets",
        &group_url(base, &h.id, "/offsets/delete"),
        &format!("{}?expiring=1", group_url(base, &h.id, "/lag")),
        html! { "Delete committed offsets of " (h.id) "?" },
        html! {
            p style="margin:0 0 16px" {
                "Forgets the offsets this group committed on a topic. The group's next consumer starts from its "
                span class="mono" { "auto.offset.reset" } ". Records are not touched."
            }
            @if v.topics.is_empty() {
                p class="help" { (h.id) " has committed no offset." }
            } @else {
                div class="field" {
                    div class="lab" { "Topic" }
                    div class="stack-y" style="gap:10px" {
                        @for (i, (t, ps)) in v.topics.iter().enumerate() {
                            @let id = format!("offset-delete-{i}");
                            label class="opt-row" for=(id) {
                                input type="checkbox" id=(id) name="topic" value=(t) checked[v.chosen.contains(t)];
                                span class="check" { (glyph(Glyph::Check, 12, "")) }
                                div {
                                    div class="t1 mono" { (t) }
                                    div class="t2" { (plural(ps.len(), "offset", "offsets")) ": " (offsets_named(t, ps)) }
                                }
                            }
                        }
                    }
                }
            }
            div style="margin-top:20px" data-extra="teaching" {
                (callout(Tone::Plain, Glyph::Info, html! {
                    b { "Why Kafka may refuse." } " A topic a member is subscribed to is refused with GROUP_SUBSCRIBED_TO_TOPIC (86). "
                    @if h.members.is_empty() { (h.id) " has no members, so nothing here is." }
                    @else if h.phase == GroupPhase::Empty { "Nothing here is." }
                    @else { (h.id) " has members, so the topics they read are." }
                }))
            }
            @if let Some(r) = refusal {
                div style="margin-top:16px" { (super::refused::refusal_callout(r)) }
            }
            (call(&v.call))
        },
        html! {
            (cancel(&format!("{}?expiring=1", group_url(base, &h.id, "/lag"))))
            button class="btn danger fill" type="submit" data-op="OffsetDelete" disabled[picked == 0] { "Delete " (plural(picked, "offset", "offsets")) }
        },
    )
}

fn offsets_named(topic: &str, ps: &[i32]) -> String {
    let shown: Vec<String> = ps.iter().take(4).map(|p| format!("{topic}-{p}")).collect();
    if ps.len() > 4 {
        format!("{}, ...", shown.join(", "))
    } else {
        shown.join(", ")
    }
}
