//! A write that Kafka refused: the pages of `group-reset-refused` and `group-delete-refused`, Kafka's own code and sentence,
//! the console's reading of the rule, and what would pass.

use maud::{Markup, html};

use super::common::{On, group_tabs, group_url, spans, title};
use crate::base::Base;
use crate::model::{GroupRefused, Refusal};
use crate::ui::button::{Size, Variant, link};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::glyph_groups::{GroupGlyph, group_glyph};
use crate::ui::layout::page_header;
use crate::ui::write::apicall;

/// Kafka's refusal as a callout, for a dialog that shows it in place.
#[must_use]
pub(super) fn refusal_callout(r: &Refusal) -> Markup {
    html! {
        div class="callout refused" data-extra="refusal" {
            (glyph(Glyph::X, 18, ""))
            div {
                span class="st refused" { "Refused" } " " span class="mono bright" { "\u{a0}" (r.name) " (" (r.code) ")" }
                p style="margin:8px 0 0" { b { "Kafka's words:" } " \"" (r.message) "\"" }
            }
        }
    }
}

fn the_refusal(r: &GroupRefused, base: &Base) -> Markup {
    html! {
        div class="callout refused" data-extra="refusal" style="margin-bottom:32px" {
            (glyph(Glyph::X, 18, ""))
            div {
                span class="st refused" { "Refused" } " " span class="mono bright" { "\u{a0}" (r.refusal.name) " (" (r.refusal.code) ")" }
                p style="margin:8px 0 0" { b { "Kafka's words:" } " \"" (r.refusal.message) "\"" }
                p style="margin:8px 0 0" { b { "The rule." } " " (spans(&r.rule, base)) }
            }
        }
    }
}

fn would_pass(r: &GroupRefused) -> Markup {
    html! {
        div class="card" {
            h2 { "What would pass" }
            div class="rules" {
                @for s in &r.steps {
                    @if s.blocking {
                        div class="r no" {
                            span class="gl" { (glyph(Glyph::Warn, 16, "")) }
                            div { (s.title) span class="sub" { (s.sub) } }
                            span class="v" { (s.value) }
                        }
                    } @else {
                        div class="r" {
                            span class="gl dim" { (glyph(Glyph::ArrowR, 16, "")) }
                            div { (s.title) span class="sub" { (s.sub) } }
                            span class="v" {}
                        }
                    }
                }
            }
        }
    }
}

fn page(r: &GroupRefused, base: &Base, on: On, back: (&str, String), tail: Markup) -> Markup {
    let h = &r.head;
    html! {
        div class="page-h" {
            div {
                (title(h))
                p class="sub" { (r.what) " was refused and nothing changed. " (r.unchanged_sentence()) }
            }
            div class="acts" { a class="btn" href=(back.1) { (group_glyph(GroupGlyph::Back, 16, "")) (back.0) } }
        }
        (group_tabs(h, base, on))
        (the_refusal(r, base))
        div class="g2" style="align-items:start" {
            (would_pass(r))
            (apicall("The call that was refused", &r.call, None))
        }
        (tail)
    }
}

impl GroupRefused {
    /// The sentence after "was refused and nothing changed.".
    fn unchanged_sentence(&self) -> String {
        if self.what == "Delete group" {
            let h = &self.head;
            format!(
                "{} is still {} with {} and its {} committed {}.",
                h.id,
                h.phase.word(),
                if h.members.len() == 1 {
                    "its member".to_owned()
                } else {
                    format!("its {} members", h.members.len())
                },
                h.commits,
                if h.commits == 1 { "offset" } else { "offsets" }
            )
        } else if self.unchanged.is_empty() {
            "The committed offsets are as they were.".to_owned()
        } else {
            format!(
                "The committed offsets are as they were: {}.",
                self.unchanged
            )
        }
    }
}

/// `group-reset-refused`.
#[must_use]
pub fn reset_refused_page(r: &GroupRefused, base: &Base) -> Markup {
    page(
        r,
        base,
        On::Lag,
        ("Back to Lag", group_url(base, &r.head.id, "/lag")),
        html! {},
    )
}

/// `group-delete-refused`.
#[must_use]
pub fn delete_refused_page(r: &GroupRefused, base: &Base) -> Markup {
    page(
        r,
        base,
        On::Members,
        ("Back to the group", group_url(base, &r.head.id, "")),
        html! { p class="help" style="margin-top:12px;max-width:none" { "When no member is left the group is Empty and the same call deletes it." } },
    )
}

/// The page for a group the broker does not know.
#[must_use]
pub fn no_group(id: &str, base: &Base) -> Markup {
    html! {
        (page_header("No such group", html! { p class="sub" { "The broker knows no group called " span class="mono" { (id) } ", or the console may not describe it. A group exists once a consumer joins or commits." } }, html! {}))
        div class="row" { (link(&base.url("/groups"), Variant::Primary, Size::Normal, Some(Glyph::ArrowR), "Back to Groups")) }
    }
}
