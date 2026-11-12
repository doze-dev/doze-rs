//! Create topic: the three steps of one route (boards `topic-create`, `topic-create-config`, `topic-create-review`) and
//! the refused state (`topic-create-refused`). The steps are one route chosen by `?step=`, and each step is a form that
//! moves on with a GET carrying every value typed so far, so the Back link and a reload keep them with no script.

use maud::{Markup, html};

use super::{field, hidden, submit};
use crate::base::{Base, segment};
use crate::format::number;
use crate::model::{CreateRefused, CreateView, Draft, Step, Teach, Template};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::layout::{card, kv, page_header};
use crate::ui::notice::{Tone, callout};
use crate::ui::write::{apicall, field_error, refused, steps, teach, terminal, text_input};
use crate::validate::retention_words;

/// The address of a step with every value of the draft in its query.
#[must_use]
pub(super) fn step_url(base: &Base, step: Step, d: &Draft) -> String {
    let q = |k: &str, v: &str| format!("{k}={}", segment(v));
    base.url(&format!(
        "/topics/new?step={}&{}&{}&{}&{}&{}",
        step.id(),
        q("name", &d.name),
        q("partitions", &d.partitions),
        q("template", d.template.id()),
        q("cleanup.policy", &d.cleanup),
        q("retention.ms", &d.retention),
    ))
}

fn hidden_draft(d: &Draft, skip: &[&str]) -> Markup {
    let all = [
        ("name", d.name.as_str()),
        ("partitions", d.partitions.as_str()),
        ("template", d.template.id()),
        ("cleanup.policy", d.cleanup.as_str()),
        ("retention.ms", d.retention.as_str()),
    ];
    let kept: Vec<_> = all.into_iter().filter(|(n, _)| !skip.contains(n)).collect();
    hidden(&kept)
}

fn under(error: Option<&String>, t: Option<&Teach>) -> Markup {
    html! {
        @if let Some(e) = error { (field_error(e)) }
        @if let Some(t) = t { (teach(t)) }
    }
}

fn starting_point(d: &Draft) -> Markup {
    const OPTIONS: [(Template, &str, &str); 3] = [
        (Template::EventLog, "Event log", "delete, keeps 7 d"),
        (
            Template::Compacted,
            "Compacted state",
            "compact: the latest value for each key",
        ),
        (Template::ShortLived, "Short lived", "delete, keeps 1 h"),
    ];
    html! {
        div class="field" {
            div class="lab" { "Start from" }
            div class="choice" {
                @for (t, name, sub) in OPTIONS {
                    @let on = d.template == t;
                    label class={"opt" @if on { " on" }} {
                        div class="opt-row" {
                            input type="radio" name="template" value=(t.id()) checked[on];
                            span class={"radio" @if on { " on" }} {}
                            div { div class="t1" { (name) } div class="t2" { (sub) } }
                        }
                    }
                }
            }
            div class="help" { "A starting point only: the next step shows every value it sets." }
        }
    }
}

fn first(v: &CreateView, base: &Base) -> Markup {
    let d = &v.draft;
    let unit = v.most.map(|m| format!("of 1 to {}", number(i64::from(m))));
    html! {
        form method="get" action=(base.url("/topics/new")) {
            (hidden(&[("step", "config")]))
            (hidden_draft(d, &["name", "partitions", "template"]))
            div class="card" { div class="form" {
                (field("n", html! { "Name" }, text_input("n", "name", &d.name, true, None, v.errors.name.is_some()), under(v.errors.name.as_ref(), Some(&v.teach_name))))
                (field("p", html! { "Partitions" }, text_input("p", "partitions", &d.partitions, true, unit.as_deref(), v.errors.partitions.is_some()), under(v.errors.partitions.as_ref(), Some(&v.teach_partitions))))
                (starting_point(d))
                div class="actions-bar" {
                    a class="btn" href=(base.url("/topics")) { "Cancel" }
                    (submit("btn pri", None, None, html! { "Next: configuration" }))
                }
            } }
        }
    }
}

fn cleanup_choice(d: &Draft, t: Option<&Teach>) -> Markup {
    const OPTIONS: [(&str, &str); 3] = [
        (
            "delete",
            "old segments go when they pass the retention time or size",
        ),
        (
            "compact",
            "keep the latest record for every key; a record with no key is refused",
        ),
        (
            "delete,compact",
            "both: compact what is kept, and drop what is too old",
        ),
    ];
    html! {
        div class="field" {
            div class="lab" { "cleanup.policy" }
            @for (value, sub) in OPTIONS {
                @let on = d.cleanup == value;
                label class="opt-row" {
                    input type="radio" name="cleanup.policy" value=(value) checked[on];
                    span class={"radio" @if on { " on" }} {}
                    div { div class="t1" { (value) } div class="t2" { (sub) } }
                }
            }
            @if let Some(t) = t { (teach(t)) }
        }
    }
}

fn second(v: &CreateView, base: &Base) -> Markup {
    let d = &v.draft;
    let words = retention_words(&d.retention);
    html! {
        form method="get" action=(base.url("/topics/new")) {
            (hidden(&[("step", "review")]))
            (hidden_draft(d, &["cleanup.policy", "retention.ms"]))
            div class="card" { div class="form" {
                (cleanup_choice(d, v.teach_cleanup.as_ref()))
                (field("r", html! { "retention.ms" }, text_input("r", "retention.ms", &d.retention, true, (!words.is_empty()).then_some(words.as_str()), v.errors.retention.is_some()), under(v.errors.retention.as_ref(), v.teach_retention.as_ref())))
                div class="actions-bar" {
                    a class="btn" href=(step_url(base, Step::Name, d)) { "Back" }
                    (submit("btn pri", None, None, html! { "Next: review" }))
                }
            } }
        }
    }
}

fn third(v: &CreateView, base: &Base) -> Markup {
    let d = &v.draft;
    let words = retention_words(&d.retention);
    let rows = vec![
        ("Name", html! { span class="mono" { (d.name) } }),
        (
            "Partitions",
            html! { (d.partitions) " " span class="sub" { "directories made and flushed before the topic is announced" } },
        ),
        (
            "Replication factor",
            html! { "1 " span class="sub" { "one broker, said once" } },
        ),
        (
            "cleanup.policy",
            html! { span class="mono" { (d.cleanup) } },
        ),
        (
            "retention.ms",
            html! { span class="mono" { (d.retention) } @if !words.is_empty() { " " span class="dim" { "(" (words.trim_start_matches("= ")) ")" } } },
        ),
        (
            "Everything else",
            html! { "the broker's defaults, none set on the topic" },
        ),
    ];
    html! {
        div class="stack-y" {
            form method="post" action=(base.url("/topics/new")) {
                (hidden_draft(d, &[]))
                (card(html! {
                    h2 { "What you will get" }
                    (kv(rows))
                    div class="actions-bar" {
                        a class="btn" href=(step_url(base, Step::Config, d)) { "Back" }
                        (submit("btn", Some("CreateTopics"), Some(("mode", "check")), html! { "Check only" }))
                        (submit("btn pri", Some("CreateTopics"), Some(("mode", "create")), html! { "Create topic" }))
                    }
                }))
            }
            @if let Some(c) = &v.checked {
                div class="banner" { (glyph(Glyph::Check, 18, "")) span { b { "Checked." } " Every check passed for " (c.partitions) " partitions, and nothing was created." } }
            }
            (callout(Tone::Plain, Glyph::Info, html! {
                b { "What can refuse this call" } br;
                "A name that already exists, a name that collides with one by " span class="mono" { "." } " and " span class="mono" { "_" }
                ", a partition count outside " (v.most.map_or_else(|| "what the broker allows".to_owned(), |m| format!("1 to {}", number(i64::from(m))))) ", a config value Kafka would not accept. Check only runs every one of those checks and creates nothing. If a client makes "
                span class="mono" { (d.name) } " while you read this, the call is refused."
            }))
        }
    }
}

/// The page at one of its three steps.
#[must_use]
pub fn create_page(v: &CreateView, base: &Base) -> Markup {
    let sub = match v.step {
        Step::Name => "Three steps. The call on the right is the one that will be sent.",
        Step::Config => {
            "What the topic does with old records. Kafka's own sentence, the default and when it takes effect sit under each field."
        }
        Step::Review => "Check what will be sent, then create. Nothing exists until you do.",
    };
    html! {
        (page_header("Create topic", html! { p class="sub" { (sub) } }, html! {}))
        (steps(v.step))
        div class="split wide" {
            @match v.step {
                Step::Name => (first(v, base)),
                Step::Config => (second(v, base)),
                Step::Review => (third(v, base)),
            }
            div class="stack-y" {
                (apicall("The call", &v.call, (v.step == Step::Name).then_some("Replication factor is always 1: this broker is the only broker.")))
                (terminal("terminal", &v.terminal))
            }
        }
    }
}

/// The refused state: Kafka's code and sentence, what can be done, and the other refusals of this call.
#[must_use]
pub fn create_refused(r: &CreateRefused, base: &Base) -> Markup {
    let most = r
        .most
        .map_or_else(String::new, |m| format!(" ({})", number(i64::from(m))));
    let rules = [
        ("INVALID_TOPIC_EXCEPTION (17)", "the name has a character Kafka forbids, or collides: \"Topic 'orders_dlq' collides with existing topic: orders.dlq\"".to_owned(), "name"),
        ("INVALID_PARTITIONS (37)", format!("zero or fewer partitions, or more than --max-partitions{most}"), "partitions"),
        ("INVALID_CONFIG (40)", "a value Kafka would not accept, naming the first bad key".to_owned(), "config"),
    ];
    html! {
        (page_header("Create topic", html! { p class="sub" { "The topic was not created." } }, html! {}))
        (steps(Step::Review))
        div class="split wide" {
            div class="stack-y" {
                (refused(&r.refusal))
                (card(html! {
                    h2 { "What you can do" }
                    div class="row wrap" style="margin-top:12px" {
                        @if let Some(name) = &r.existing { a class="btn pri" href=(super::topic_url(base, name, "")) { "Open the existing topic" } }
                        a class="btn" href=(step_url(base, Step::Name, &r.draft)) { "Choose another name" }
                        a class="btn" href=(base.url("/topics")) { "Back to topics" }
                    }
                }))
                (card(html! {
                    h2 { "The other refusals of this call" }
                    p class="lede" { "Each topic in a CreateTopics is answered on its own, with Kafka's code." }
                    div class="rules" {
                        @for (code, why, tag) in &rules {
                            div class="r no" { (glyph(Glyph::Warn, 16, "")) div { span class="mono" { (code) } span class="sub" { (why) } } span class="v" { (tag) } }
                        }
                    }
                }))
            }
            div class="stack-y" { (apicall("The answer", &r.answer, Some("A refused topic answers -1 partitions and -1 replication factor."))) }
        }
    }
}
