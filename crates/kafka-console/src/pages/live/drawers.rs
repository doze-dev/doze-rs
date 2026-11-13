//! A request in full, as a drawer over the Live page (boards `request-detail`, `request-refused`, `request-fenced` and
//! `request-too-large`; the refusals are in `refusals.rs`). Each body is a pure function of the request and of what the broker
//! says now, with its own heading id; [`request_drawer`] puts one in the overlay frame (`ui::overlay`), over the Live page.

use maud::{Markup, html};

use super::commands::fetch_commands;
use super::parts::{class_css, class_word};
use super::refusals;
use crate::base::{Base, segment};
use crate::format::{bytes, clock_ms, number};
use crate::model::{WireClass, WireDetail};
use crate::ui::button::{Size, Variant, link};
use crate::ui::client;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::overlay::{Kind as OverlayKind, frame};

/// What the broker says about the topic now, read when the drawer opens: the rules of a refusal are shown against it, and
/// the page says "now" because the topic may have changed since the request.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "RequestLimits"))]
pub struct Limits {
    pub cleanup: Option<String>,
    /// `max.message.bytes` in force, and whether the topic sets it (else the broker's value applies).
    pub max_message_bytes: Option<(u64, bool)>,
}

/// Which drawer a request opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "RequestKind"))]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Detail,
    Refused,
    Fenced,
    TooLarge,
}

impl Kind {
    /// The heading's id, which names the panel.
    #[must_use]
    pub const fn heading_id(self) -> &'static str {
        match self {
            Self::Detail => "request-detail",
            Self::Refused => "request-refused",
            Self::Fenced => "request-fenced",
            Self::TooLarge => "request-too-large",
        }
    }

    /// The drawer a request calls for, by what became of it.
    #[must_use]
    pub fn of(d: &WireDetail) -> Self {
        match (d.row.class, d.row.code) {
            (WireClass::Done | WireClass::Told, _) => Self::Detail,
            (_, 47 | 90) => Self::Fenced,
            (_, 10) => Self::TooLarge,
            _ => Self::Refused,
        }
    }
}

/// The heading id of a request's drawer.
#[must_use]
pub fn detail_heading_id(d: &WireDetail) -> &'static str {
    Kind::of(d).heading_id()
}

/// The top of every drawer: the heading, the one-line state, Close.
pub(super) fn head(id: &str, title: Markup, mono: bool, d: &WireDetail, close: &str) -> Markup {
    let r = &d.row;
    html! {
        div class="row sb" style="align-items:flex-start" {
            div {
                h2 id=(id) class=[mono.then_some("mono")] { (title) }
                div class="sent" {
                    span class={"st " (class_css(r.class))} { (class_word(r.class, r.code)) }
                    " · " (r.client) @if let Some(t) = &r.tag { " (" (t) ")" }
                    " · " span data-vary { (clock_ms(r.at_ms)) }
                }
            }
            a class="btn sm ghost" href=(close) data-dialog-close data-ui="close" aria-label="Close" { (glyph(Glyph::X, 16, "")) "Close" }
        }
        div class="hr" style="margin:16px 0 20px" {}
    }
}

fn kv_row(label: &str, value: Markup) -> Markup {
    html! { dt { (label) } dd { (value) } }
}

fn asked(d: &WireDetail) -> Markup {
    html! {
        @if d.topics.is_empty() {
            @if let Some(w) = &d.what { (w) } @else if let Some(g) = &d.row.group { span class="mono" { (g) } } @else { "-" }
        } @else {
            @for (t, parts) in &d.topics {
                span class="mono" { (t) @if !parts.is_empty() { "-" (parts.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")) } }
                " "
            }
            @if let Some(o) = d.from_offset { span class="sub" { "from offset " (number(o)) } }
        }
    }
}

fn answer(d: &WireDetail) -> Markup {
    html! {
        (d.row.detail)
        span class="sub" { "error_code " (d.row.code) " (" (d.row.code_name) ")" @if d.response_bytes > 0 { " · " (bytes(u64::from(d.response_bytes))) " on the wire" } }
    }
}

fn took(d: &WireDetail) -> Markup {
    let us = d.row.took_us;
    let ms = if us < 1000 {
        "<1 ms".to_owned()
    } else {
        format!("{} ms", us / 1000)
    };
    html! {
        span class="mono" { (ms) }
        @if let Some(w) = d.wait_ms.filter(|w| *w > 0) {
            span class="sub" { "The client was willing to wait " (number(i64::from(w))) " ms for records: a long poll, answered when data came or the wait ended." }
        }
        @if let Some((hi, lo, med)) = d.took_spread {
            span class="sub" { "Folded: " (d.row.count) " requests; slowest " (hi / 1000) " ms, fastest " (lo / 1000) " ms, median " (med / 1000) " ms." }
        }
    }
}

/// The rows every drawer lists under its state.
fn facts(d: &WireDetail) -> Markup {
    html! {
        dl class="kv sm" {
            (kv_row("Client", html! {
                span class="mono" { (d.row.client) }
                span class="sub" { (d.software.as_deref().unwrap_or("software not announced")) @if let Some(p) = &d.peer { " · " (p) } " · " (d.principal) }
            }))
            (kv_row("Asked for", asked(d)))
            @if let Some(g) = &d.row.group { (kv_row("Group", html! { span class="mono" { (g) } @if let Some(m) = &d.member { span class="sub" { "member " (m) @if let Some(n) = d.generation { ", generation " (n) } } } })) }
            @if let Some(t) = &d.txn_id { (kv_row("Transaction", html! { span class="mono" { (t) } @if let Some((p, e)) = d.producer { span class="sub" { "producer " (p) ", epoch " (e) } } })) }
            @if let Some(r) = &d.reason { (kv_row("Reason", html! { (r) span class="sub" { "the client's own words, clipped" } })) }
            (kv_row("Answer", answer(d)))
            (kv_row("Took", took(d)))
            (kv_row("Correlation id", html! { span class="mono" { (number(i64::from(d.correlation))) } }))
        }
    }
}

fn parts(d: &WireDetail) -> Markup {
    if d.parts.len() < 2 {
        return html! {};
    }
    html! {
        h3 style="margin:24px 0 8px;font-size:14px" { "Per partition" }
        table class="t" { thead { tr { th scope="col" { "Partition" } th scope="col" { "Result" } th scope="col" class="r" { "Offset" } } }
            tbody { @for p in &d.parts { tr {
                td class="mono" { (p.topic) "-" (p.partition) }
                td { @if p.code == 0 { "Done" } @else { span class="mono" { (p.code_name) } } }
                td class="mono r" { @if let Some(o) = p.offset { (number(o)) } @else { "-" } }
            } } } }
        @if d.parts_more > 0 { p class="help" { "And " (d.parts_more) " more, not kept." } }
    }
}

fn copy_as(d: &WireDetail, listen: &str) -> Markup {
    let Some((t, p, o)) = fetch_target(d) else {
        return html! {};
    };
    let c = fetch_commands(listen, &t, p, o);
    html! {
        h3 style="margin:24px 0 8px;font-size:14px" data-extra="copy-as" { "Copy as" }
        div class="stack-y" style="gap:12px" {
            @for (i, (label, cmd)) in [("kcat", &c.kcat), ("Kafka console consumer", &c.console)].into_iter().enumerate() {
                div {
                    div class="row sb" style="margin-bottom:6px" { span class="dim" { (label) } (client::copy_from_button("btn sm", &format!("copy-{i}"), html! { (glyph(Glyph::Copy, 16, "")) "Copy" })) }
                    (crate::ui::code::pre(cmd, Some(&format!("copy-{i}")), None))
                }
            }
        }
    }
}

/// The topic, partition and offset a Fetch row asked for, when it names exactly one.
fn fetch_target(d: &WireDetail) -> Option<(String, i32, i64)> {
    if d.row.api != "Fetch" {
        return None;
    }
    let (t, parts) = d.topics.first()?;
    Some((t.clone(), *parts.first()?, d.from_offset?))
}

/// The drawer of a request that was answered (`request-detail`).
#[must_use]
pub fn detail_body(d: &WireDetail, base: &Base, listen: &str, close: &str) -> Markup {
    let r = &d.row;
    let title = html! { (r.api) " v" (r.version) @if r.transactional { " (txn)" } };
    let open = d.topics.first().map(|(t, _)| t.clone());
    html! {
        (head(Kind::Detail.heading_id(), title, false, d, close))
        (facts(d))
        (parts(d))
        div class="teach" style="margin-top:20px" data-extra="api-preview" {
            b { "What the broker kept." } " "
            span class="dim" { "This summary only. Request bodies and record bytes are not recorded: to see the record, open the topic at that offset." }
        }
        (copy_as(d, listen))
        div class="row" style="margin-top:24px;gap:10px" {
            @if let Some(t) = &open {
                (link(&base.url(&format!("/topics/{}", segment(t))), Variant::Primary, Size::Normal, Some(Glyph::Topic), &format!("Open {t}")))
            }
            @if let Some((t, p, o)) = fetch_target(d) {
                a class="btn" href={(base.url(&format!("/topics/{}/messages", segment(&t)))) "?partition=" (p) "&offset=" (o) "&limit=1"} data-op="Fetch" { (glyph(Glyph::Refresh, 16, "")) "Fetch it again" }
            }
        }
    }
}

/// The body of the drawer a request opens, chosen by what became of it.
#[must_use]
pub fn request_body(
    d: &WireDetail,
    limits: &Limits,
    base: &Base,
    listen: &str,
    close: &str,
) -> Markup {
    match Kind::of(d) {
        Kind::Detail => detail_body(d, base, listen, close),
        Kind::Refused => refusals::refused_body(d, limits, base, listen, close),
        Kind::Fenced => refusals::fenced_body(d, base, close),
        Kind::TooLarge => refusals::too_large_body(d, limits, base, close),
    }
}

/// A request's drawer over the Live page: the overlay element (`ui::overlay`), which closes to `close`, the Live page's own
/// address with the filters it had.
#[must_use]
pub fn request_drawer(
    d: &WireDetail,
    limits: &Limits,
    base: &Base,
    listen: &str,
    close: &str,
) -> Markup {
    let kind = Kind::of(d);
    frame(
        OverlayKind::Drawer,
        kind.heading_id(),
        close,
        request_body(d, limits, base, listen, close),
    )
}
