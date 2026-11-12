//! The parts the topic forms share: the steps bar, the teaching under a field, the call beside a form, a refusal, and
//! the text field. Everything shown is text and is escaped; nothing is HTML that came from a request or the broker.

use maud::{Markup, html};

use super::client;
use super::code::block;
use super::glyph::{Glyph, glyph};
use super::glyph_extra::{Extra, glyph_extra};
use crate::model::{Lines, Refusal, Span, Step, Teach};

/// Spans of a sentence: plain, monospace, bold.
#[must_use]
pub fn spans(parts: &[Span]) -> Markup {
    html! {
        @for p in parts {
            @match p {
                Span::Text(t) => (t),
                Span::Mono(t) => span class="mono" { (t) },
                Span::Strong(t) => b { (t) },
            }
        }
    }
}

/// The three steps of the Create-topic form, the current one marked and the ones before it done.
#[must_use]
pub fn steps(current: Step) -> Markup {
    const NAMES: [(Step, &str); 3] = [
        (Step::Name, "Name and partitions"),
        (Step::Config, "Configuration"),
        (Step::Review, "Review and create"),
    ];
    let place = |s: Step| NAMES.iter().position(|(x, _)| *x == s).unwrap_or(0);
    let now = place(current);
    html! {
        div class="steps" {
            @for (i, (_, label)) in NAMES.iter().enumerate() {
                @if i > 0 { span class="bar" {} }
                @if i < now {
                    span class="s done" { i { (glyph(Glyph::Check, 12, "")) } (label) }
                } @else if i == now {
                    span class="s on" { i { (i + 1) } (label) }
                } @else {
                    span class="s " { i { (i + 1) } (label) }
                }
            }
        }
    }
}

/// The lines under a field: what Kafka says, the default, the range, when it takes effect, and what is so here.
#[must_use]
pub fn teach(t: &Teach) -> Markup {
    html! {
        div class="teach" data-extra="teaching" {
            div { b { "Kafka says" } " " (t.says) }
            @if !t.default.is_empty() { div { b { "Default" } " " (spans(&t.default)) } }
            @if !t.range.is_empty() { div { b { "Range" } " " (spans(&t.range)) } }
            div { b { "Takes effect" } " " (spans(&t.takes_effect)) }
            div { b { "Here" } " " (spans(&t.here)) }
        }
    }
}

/// The call (or the answer) beside a form, as the boards draw it: a titled block of code and a line under it.
#[must_use]
pub fn apicall(title: &str, lines: &Lines, note: Option<&str>) -> Markup {
    html! {
        div class="apicall" data-extra="api-preview" {
            div class="hd" { (glyph_extra(Extra::Api, 14)) (title) }
            (block(lines, None, None))
            @if let Some(n) = note { p class="help" style="margin:12px 0 0" { (n) } }
        }
    }
}

/// "Or from a terminal": the same call as a command, with a Copy button where a script can do it.
#[must_use]
pub fn terminal(id: &str, lines: &[String]) -> Markup {
    html! {
        div class="apicall" data-extra="copy-as" style="margin-top:16px" {
            div class="hd" { (glyph(Glyph::Code, 14, "")) "Or from a terminal" }
            pre class="code" id=(id) { span class="c" { "# the same, with Kafka's own tool" } "\n" (lines.join("\n")) }
            div class="row" style="margin-top:12px" {
                (client::copy_from_button("btn sm", id, html! { (glyph(Glyph::Copy, 14, "")) "Copy" }))
            }
        }
    }
}

/// A refused call: Kafka's code and the broker's sentence, then the console's reading and what would pass.
#[must_use]
pub fn refused(r: &Refusal) -> Markup {
    html! {
        div class="callout refused" data-extra="refusal" {
            (glyph(Glyph::Warn, 18, ""))
            div {
                b { "Refused · " (r.name) " (" (r.code) ")" }
                br; (r.message)
                @if let Some(rule) = &r.rule { br; span class="dim" { "Rule: " (spans(rule)) } }
                @if let Some(pass) = &r.would_pass { br; b { "What would pass" } " " (spans(pass)) }
            }
        }
    }
}

/// A text input in the kit's box: `unit` is the grey words at its right, `err` marks it.
#[must_use]
pub fn text_input(
    id: &str,
    name: &str,
    value: &str,
    mono: bool,
    unit: Option<&str>,
    err: bool,
) -> Markup {
    let class = format!(
        "input{}{}",
        if mono { " mono" } else { "" },
        if err { " err" } else { "" }
    );
    html! {
        div class=(class) {
            input id=(id) name=(name) type="text" value=(value) autocomplete="off" spellcheck="false" aria-invalid=[err.then_some("true")];
            @if let Some(u) = unit { span class="unit" { (u) } }
        }
    }
}

/// The error line under a field.
#[must_use]
pub fn field_error(text: &str) -> Markup {
    html! { div class="help err" { (text) } }
}

#[cfg(test)]
mod tests;
