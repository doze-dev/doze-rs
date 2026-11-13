//! Produce to a topic: the dialog (board `topic-produce`) and the refused state (board `topic-produce-keyless`).

use maud::{Markup, html};

use super::{field, opt, submit, topic_url};
use crate::base::Base;
use crate::model::{ProduceRefused, ProduceView};
use crate::ui::components::{Choice, Select, select};
use crate::ui::dialog::{cancel, modal};
use crate::ui::layout::card;
use crate::ui::write::{apicall, field_error, refused, teach, text_input};
use crate::write_words::acks_teach;

/// The most header rows the dialog offers, filled ones and a blank one.
const MOST_HEADERS: usize = 8;

fn partition_select(v: &ProduceView) -> Markup {
    let values: Vec<String> = (0..v.partitions).map(|p| p.to_string()).collect();
    let mut choices = vec![Choice::new("", "auto").desc("the producer's partitioner picks")];
    choices.extend(values.iter().map(|p| Choice::new(p, p)));
    select(&Select {
        class: "dz-block mono",
        ..Select::new("pp", "partition", "Partition", &choices, &v.partition)
    })
}

fn header_rows(v: &ProduceView) -> Markup {
    let mut rows: Vec<(String, String)> = v
        .headers
        .iter()
        .filter(|(n, _)| !n.is_empty())
        .cloned()
        .collect();
    rows.truncate(MOST_HEADERS - 1);
    rows.push((String::new(), String::new()));
    html! {
        div class="field" {
            label for="hn0" { "Header" (opt(&v.headers.iter().filter(|(n, _)| !n.is_empty()).count().to_string())) }
            @for (i, (n, val)) in rows.iter().enumerate() {
                div class="g2" style=(if i == 0 { "gap:16px" } else { "gap:16px;margin-top:8px" }) {
                    div class="input mono" { input id={"hn" (i)} name={"hn" (i)} type="text" value=(n) aria-label={"Header name " (i + 1)} autocomplete="off"; }
                    div class="input mono" { input id={"hv" (i)} name={"hv" (i)} type="text" value=(val) aria-label={"Header value " (i + 1)} autocomplete="off"; }
                }
            }
        }
    }
}

fn acks_choice(v: &ProduceView) -> Markup {
    html! {
        div class="field" {
            div class="lab" { "acks" }
            div class="seg" style="align-self:flex-start" {
                @for (value, label) in [("-1", "all (-1)"), ("1", "1"), ("0", "0")] {
                    label { input type="radio" name="acks" value=(value) checked[v.acks == value]; (label) }
                }
            }
            @if let Some(e) = &v.errors.acks { (field_error(e)) }
            (teach(&acks_teach(v.transaction.as_deref())))
        }
    }
}

fn key_help(v: &ProduceView) -> Markup {
    html! {
        div class="help" {
            span class="mono" { (v.topic) }
            @if v.compacts {
                " keeps the latest record for every key (cleanup.policy " (v.cleanup) "), so a record with no key is refused (INVALID_RECORD). A key with no value is a deletion."
            } @else {
                " keeps its records by " (v.cleanup) ", so a record with no key is accepted. On a compacted topic it is refused. Leave Value empty for a deletion (a key with no value)."
            }
        }
    }
}

fn body(v: &ProduceView) -> Markup {
    let key_label = if v.compacts {
        "required on this topic"
    } else {
        "optional on this topic"
    };
    html! {
        div class="form" style="gap:18px" {
            div class="g2" style="gap:16px" {
                (field("pp", html! { "Partition" }, partition_select(v), html! {
                    @if let Some(e) = &v.errors.partition { (field_error(e)) } @else { div class="help" { "Auto, or 0 to " (v.partitions - 1) ". Auto sends a key where Kafka's default partitioner does." } }
                }))
                (field("k", html! { "Key" (opt(key_label)) }, text_input("k", "key", &v.key, true, None, v.errors.key.is_some()), html! {
                    @if let Some(e) = &v.errors.key { (field_error(e)) }
                }))
            }
            (field("v", html! { "Value" }, html! {
                div class="input ta mono" { textarea id="v" name="value" rows="3" spellcheck="false" { (v.value) } }
            }, html! {}))
            (header_rows(v))
            (acks_choice(v))
            (key_help(v))
            (apicall("The call", &v.call, None))
        }
    }
}

/// The dialog, over the topic's Messages page.
#[must_use]
pub fn produce_dialog(v: &ProduceView, base: &Base) -> Markup {
    let action = topic_url(base, &v.topic, "/produce");
    modal(
        "produce",
        &action,
        &topic_url(base, &v.topic, ""),
        html! { "Produce to " span class="mono" { (v.topic) } },
        body(v),
        html! {
            (cancel(&topic_url(base, &v.topic, "")))
            (submit("btn pri", Some("Produce"), None, html! { "Send" }))
        },
    )
}

/// The address of the dialog with what was typed, so that "Back to Produce" returns to the same record.
fn back_url(r: &ProduceRefused, base: &Base) -> String {
    let v = &r.view;
    let q = |k: &str, val: &str| format!("{k}={}", crate::base::segment(val));
    let mut parts = vec![
        q("partition", &v.partition),
        q("key", &v.key),
        q("value", &v.value),
        q("acks", &v.acks),
    ];
    for (i, (n, val)) in v.headers.iter().filter(|(n, _)| !n.is_empty()).enumerate() {
        parts.push(q(&format!("hn{i}"), n));
        parts.push(q(&format!("hv{i}"), val));
    }
    let query = parts.join("&");
    // A record too large to put in an address comes back empty rather than as a request no server will take.
    let base_url = topic_url(base, &v.topic, "/produce");
    if query.len() > 4000 {
        base_url
    } else {
        format!("{base_url}?{query}")
    }
}

/// The refused state: the record as sent, Kafka's code and the broker's sentence, and the answer.
#[must_use]
pub fn produce_refused(r: &ProduceRefused, base: &Base) -> Markup {
    let v = &r.view;
    let sentence = if v.compacts {
        "A compacted topic keeps the latest record for every key. Without a key a record has nothing to be the latest of."
    } else {
        "The record was not written. No offset was used."
    };
    html! {
        div class="page-h" { div {
            div class="title-row" { h1 class="mono" { (v.topic) } span class="pill mono" { (v.cleanup) } }
            p class="sent" style="margin:10px 0 0" { (sentence) }
        } }
        div class="split wide" {
            div class="stack-y" {
                (card(html! {
                    h2 { "Produce to " (v.topic) }
                    div class="form" style="margin-top:16px;gap:18px" {
                        (field("k", html! { "Key" (opt(if v.compacts { "required on this topic" } else { "optional on this topic" })) }, html! {
                            div class={"input mono" @if v.errors.key.is_some() { " err" }} { input id="k" type="text" value=(v.key) placeholder="empty" readonly; }
                        }, html! { @if let Some(e) = &v.errors.key { (field_error(e)) } }))
                        (field("v", html! { "Value" }, html! {
                            div class="input mono" { input id="v" type="text" value=(v.value) readonly aria-label="Value"; }
                        }, html! {}))
                        div class="row" {
                            button class="btn pri dis" type="button" data-op="Produce" disabled { "Send" }
                            span class="dim" { "Sent once: the broker answered below." }
                        }
                    }
                }))
                (refused(&r.refusal))
                @for e in &r.record_errors { p class="help" { "The broker said of the record: " (e) } }
            }
            div class="stack-y" {
                (apicall("The answer", &r.answer, None))
                a class="btn" href=(back_url(r, base)) { "Back to Produce" }
            }
        }
    }
}
