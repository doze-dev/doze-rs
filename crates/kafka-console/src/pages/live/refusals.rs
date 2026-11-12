//! The three refusal drawers: `request-refused` (Kafka's own words and the rule behind them, and the amber Denied class of the
//! ACLs), `request-fenced` (a producer behind the epoch it was given) and `request-too-large` (a batch over the limit).
//!
//! The rules are shown against what the broker says NOW (the drawer reads the topic's config when it opens): a topic can have
//! changed since the request, and the page says so. What the recorder kept of the request itself (never a record) is what the
//! per-request lines come from.

use maud::{Markup, html};

use super::commands::keyed_produce;
use super::drawers::{Kind, Limits, head};
use crate::base::{Base, segment};
use crate::format::{bytes, number};
use crate::model::{WireClass, WireDetail};
use crate::ui::button::{Size, Variant, link};
use crate::ui::client;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::notice::{Tone, callout};

/// One line of the rules list: passed or not, what was checked, the detail, and the tag at the right.
struct Rule {
    ok: bool,
    title: String,
    sub: String,
    tag: String,
}

fn rules(list: &[Rule]) -> Markup {
    html! {
        div class="rules" { @for r in list {
            div class={"r " (if r.ok { "ok" } else { "no" })} style=[(!r.ok).then_some("--warn:var(--refused)")] {
                (glyph(if r.ok { Glyph::Check } else { Glyph::X }, 16, ""))
                div { (r.title) span class="sub" { (r.sub) } }
                span class="v" { (r.tag) }
            }
        } }
    }
}

fn h3(text: &str, extra: Option<&str>) -> Markup {
    html! { h3 style="margin:24px 0 4px;font-size:14px" data-extra=[extra] { (text) } }
}

/// The broker's own words: its message when it gave one, else Kafka's words for the code.
fn words(d: &WireDetail) -> Markup {
    let own = d.record_note.as_ref().or(d.note.as_ref());
    html! { b { "Kafka's words" } br; (own.map_or(d.words.as_str(), String::as_str)) }
}

fn who_when(d: &WireDetail, written: bool) -> Markup {
    html! {
        dl class="kv sm" style="margin-top:8px" {
            dt { "Client" } dd { span class="mono" { (d.row.client) } @if let Some(s) = &d.software { " " span class="dim" { "(" (s) ")" } } }
            dt { "At" } dd class="mono" data-vary { (crate::format::clock_ms(d.row.at_ms)) " · " (d.row.took_us / 1000) " ms" }
            dt { "Written" } dd { @if written { "Part of it" } @else { "Nothing" } }
        }
    }
}

fn topic_of(d: &WireDetail) -> Option<&str> {
    d.topics.first().map(|(t, _)| t.as_str())
}

fn actions(d: &WireDetail, base: &Base) -> Markup {
    let Some(t) = topic_of(d) else {
        return html! {};
    };
    let t = segment(t);
    html! {
        div class="row wrap" style="margin-top:20px;gap:10px" {
            (link(&base.url(&format!("/topics/{t}/produce")), Variant::Primary, Size::Normal, Some(Glyph::Send), "Produce with a key"))
            (link(&base.url(&format!("/topics/{t}/config")), Variant::Plain, Size::Normal, None, "Config"))
            (link(&base.url(&format!("/topics/{t}/log")), Variant::Plain, Size::Normal, None, "The log"))
        }
    }
}

fn denied_rules(d: &WireDetail) -> Vec<Rule> {
    vec![
        Rule {
            ok: true,
            title: format!("The login is {}", d.principal),
            sub: "the connection's principal, from its login".into(),
            tag: "who".into(),
        },
        Rule {
            ok: false,
            title: "An ACL allows this".into(),
            sub: d.plain.clone(),
            tag: "denied".into(),
        },
    ]
}

fn invalid_record_rules(d: &WireDetail, limits: &Limits) -> Vec<Rule> {
    let mut out = Vec::new();
    if let (Some(t), Some(c)) = (topic_of(d), &limits.cleanup) {
        out.push(Rule {
            ok: true,
            title: "The topic's cleanup policy, as of now".into(),
            sub: format!("{t} cleanup.policy={c}"),
            tag: "config".into(),
        });
    }
    let said = d
        .record_note
        .as_ref()
        .or(d.note.as_ref())
        .cloned()
        .unwrap_or_else(|| d.words.clone());
    out.push(Rule {
        ok: false,
        title: "Every record passes the topic's rules".into(),
        sub: said,
        tag: "refused".into(),
    });
    out
}

/// The generic refusal: Kafka's own, the amber Denied of an ACL or a login, a failure, or a closed connection.
#[must_use]
pub fn refused_body(
    d: &WireDetail,
    limits: &Limits,
    base: &Base,
    listen: &str,
    close: &str,
) -> Markup {
    let r = &d.row;
    let denied = r.class == WireClass::Denied;
    let closed = r.class == WireClass::Closed;
    let title = html! { (r.code_name) " (" (r.code) ")" };
    let list = if denied {
        denied_rules(d)
    } else if r.code == 87 {
        invalid_record_rules(d, limits)
    } else {
        vec![Rule {
            ok: false,
            title: d.plain.clone(),
            sub: d.words.clone(),
            tag: if denied { "denied" } else { "refused" }.into(),
        }]
    };
    let compacted = limits
        .cleanup
        .as_deref()
        .is_some_and(|c| c.contains("compact"));
    html! {
        (head(Kind::Refused.heading_id(), if closed { html! { "Connection closed" } } else { title }, !closed, d, close))
        (callout(if denied { Tone::Warn } else if r.class == WireClass::Failed || closed { Tone::Bad } else { Tone::Refused }, Glyph::Warn, if closed { html! { b { "Why" } br; (r.detail) } } else { words(d) }))
        (h3("The rule", Some("teaching")))
        p class="help" style="margin:0 0 6px" { (d.plain) }
        (rules(&list))
        @if !d.pass.is_empty() {
            (h3("What would pass", None))
            p class="help" style="margin:0" { (d.pass) }
        }
        @if r.code == 87 && compacted {
            @if let Some(t) = topic_of(d) {
                div class="row sb" style="margin:12px 0 6px" { span class="dim" { "Give it a key" } (client::copy_from_button("btn sm", "copy-keyed", html! { (glyph(Glyph::Copy, 16, "")) "Copy" })) }
                (crate::ui::code::pre(&keyed_produce(listen, t, &r.client), Some("copy-keyed"), None))
                p class="help" { "A key with an empty value is also a valid record: a tombstone, which deletes that key at compaction." }
            }
        }
        (who_when(d, false))
        @if !denied && !closed { (actions(d, base)) }
    }
}

/// The producer behind its epoch: the requests that led to it, and the two epochs side by side.
#[must_use]
pub fn fenced_body(d: &WireDetail, base: &Base, close: &str) -> Markup {
    let r = &d.row;
    let sent = d.producer;
    let current = d
        .story
        .iter()
        .rev()
        .find(|s| s.api == "InitProducerId" && s.class == WireClass::Done)
        .map(|s| s.id);
    html! {
        (head(Kind::Fenced.heading_id(), html! { (r.code_name) " (" (r.code) ")" }, true, d, close))
        (callout(Tone::Refused, Glyph::Warn, words(d)))
        (h3("How it came to this", None))
        @if d.story.is_empty() {
            p class="help" { "No earlier request of this transactional id or producer is on the wire any more (the recorder keeps a bounded tape), so the story is only this request." }
        } @else {
            div class="story" data-extra="story" {
                @for s in &d.story {
                    div class="ev" { div class="tm" data-vary { (crate::format::clock(s.at_ms)) } div class="dot" { i {} }
                        div class="tx" { b { (s.api) } " " span class="mono" { "v" (s.version) } " " (s.detail) } }
                }
                div class="ev bad" { div class="tm" data-vary { (crate::format::clock(r.at_ms)) } div class="dot" { i {} }
                    div class="tx" { "A request arrived from " span class="mono" { (r.client) }
                        @if let Some((_, e)) = sent { " carrying " b { "epoch " (e) } } ": refused, and nothing written." } }
            }
        }
        (rules(&[
            Rule { ok: true, title: "The broker knows this producer".into(), sub: current.map_or_else(|| "from the transactional id's last InitProducerId".to_owned(), |_| "its newest InitProducerId is above".to_owned()), tag: sent.map_or_else(String::new, |(p, _)| format!("producer {p}")) },
            Rule { ok: false, title: "The request's epoch is current".into(), sub: sent.map_or_else(String::new, |(_, e)| format!("it sent {e}, an epoch the broker has since replaced")), tag: sent.map_or_else(String::new, |(_, e)| format!("epoch {e}")) },
        ]))
        (h3("What would pass", None))
        p class="help" style="margin:0" { "The producer asks again with " span class="mono" { "InitProducerId" } " for its transactional id, gets a newer epoch and starts at sequence 0. Clients do this on their own after the error." }
        (who_when(d, false))
        @if let Some(t) = topic_of(d) {
            div class="row wrap" style="margin-top:20px;gap:10px" {
                (link(&base.url(&format!("/topics/{}/producers", segment(t))), Variant::Primary, Size::Normal, None, "How fencing works: the producers of this topic"))
            }
        }
    }
}

/// A batch over the size limit.
#[must_use]
pub fn too_large_body(d: &WireDetail, limits: &Limits, base: &Base, close: &str) -> Markup {
    let r = &d.row;
    let sent = u64::from(d.request_bytes);
    let topic = topic_of(d).unwrap_or("the topic");
    let list = match limits.max_message_bytes {
        Some((limit, topic_sets)) => vec![
            Rule {
                ok: false,
                title: "Batch size is within the limit".into(),
                sub: format!(
                    "{} B sent, {} B over (the whole request, as of the limit now)",
                    number(i64::try_from(sent).unwrap_or(0)),
                    number(i64::try_from(sent.saturating_sub(limit)).unwrap_or(0))
                ),
                tag: format!("{} B", number(i64::try_from(limit).unwrap_or(0))),
            },
            Rule {
                ok: true,
                title: "Where the limit comes from".into(),
                sub: if topic_sets {
                    format!("{topic} sets its own max.message.bytes")
                } else {
                    format!("{topic} sets none: the broker value applies")
                },
                tag: if topic_sets { "topic" } else { "broker" }.into(),
            },
        ],
        None => vec![Rule {
            ok: false,
            title: "Batch size is within the limit".into(),
            sub: format!("{} sent; the limit could not be read now", bytes(sent)),
            tag: "limit".into(),
        }],
    };
    html! {
        (head(Kind::TooLarge.heading_id(), html! { (r.code_name) " (" (r.code) ")" }, true, d, close))
        (callout(Tone::Refused, Glyph::Warn, html! { b { "Kafka's words" } br; (d.words) br;
            span class="mono" style="font-size:12.5px" { (number(i64::try_from(sent).unwrap_or(0))) " B request" @if let Some((l, _)) = limits.max_message_bytes { " against max.message.bytes " (number(i64::try_from(l).unwrap_or(0))) } "." } }))
        (h3("The rule", Some("teaching")))
        p class="help" style="margin:0 0 6px" { "A batch larger than the limit is refused whole. The topic's own " span class="mono" { "max.message.bytes" } " wins; with none set, the broker's " span class="mono" { "message.max.bytes" } " applies." }
        (rules(&list))
        (h3("What would pass", None))
        p class="help" style="margin:0" { (d.pass) }
        (who_when(d, false))
        div class="row wrap" style="margin-top:20px;gap:10px" {
            @if let Some(t) = topic_of(d) {
                (link(&base.url(&format!("/topics/{}/config", segment(t))), Variant::Primary, Size::Normal, None, "Raise max.message.bytes"))
                (link(&base.url(&format!("/topics/{}", segment(t))), Variant::Plain, Size::Normal, None, &format!("Open {t}")))
            }
        }
    }
}
