//! The saved state (board `config-saved`): what the broker answered, each config before and after as DescribeConfigs
//! says them on each side of the call, and when the new value counts.
//!
//! The board's "61 % of the log is dirty ... not due" verdict needs the cleaner's own state, which a console may not
//! read (debt J121); the card says what the broker does with the value and when, which is all the broker can say.

use maud::{Markup, html};

use super::common::{config_tabs, config_url, page_head, source_tag};
use crate::base::Base;
use crate::config_effects::{Moment, moment, reader, sees, sentence, when_label};
use crate::format::number;
use crate::model::{ConfigChange, ConfigPace, ConfigSaved, ConfigValue, Span};
use crate::ui::button::{Size, Variant, link};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::layout::card;

fn names(changes: &[ConfigChange]) -> Vec<Span> {
    let mut out = Vec::new();
    for (i, c) in changes.iter().enumerate() {
        if i > 0 {
            out.push(Span::text(" and "));
        }
        out.push(Span::strong(c.name.clone()));
    }
    out
}

fn values(changes: &[ConfigChange]) -> Vec<Span> {
    let mut out = Vec::new();
    for (i, c) in changes.iter().enumerate() {
        if i > 0 {
            out.push(Span::text(" and "));
        }
        let now = c.now.as_ref().map_or("not set", |n| n.value.as_str());
        out.push(Span::strong(now.to_owned()));
    }
    out
}

/// The sentence under the name.
fn sentence_of(s: &ConfigSaved) -> Vec<Span> {
    let first = s.changes.first().map(|c| c.name.as_str()).unwrap_or("");
    let tail = moment(first).map_or_else(String::new, |m| sees(m, s.pace));
    let others = s.total.saturating_sub(s.changes.len());
    let mut out = names(&s.changes);
    out.push(Span::text(if s.changes.len() == 1 {
        " is now "
    } else {
        " are now "
    }));
    out.extend(values(&s.changes));
    out.push(Span::text(format!(
        ", set on this topic. The other {} configs are unchanged; {tail}.",
        number(i64::try_from(others).unwrap_or(0))
    )));
    out
}

fn value_cell(v: Option<&ConfigValue>) -> Markup {
    html! {
        td class="mono" {
            @if let Some(v) = v { (v.value) span class="sub" { (source_tag(v.source)) } } @else { "not set" }
        }
    }
}

fn before_after(changes: &[ConfigChange]) -> Markup {
    card(html! {
        h2 { "Before and after" }
        table class="t" style="margin-top:12px" {
            thead { tr { th scope="col" { "Config" } th scope="col" { "Before" } th scope="col" { "Now" } } }
            tbody {
                @for c in changes {
                    tr {
                        td class="mono bright" { (c.name) }
                        (value_cell(c.before.as_ref()))
                        (value_cell(c.now.as_ref()))
                    }
                }
            }
        }
    })
}

fn story(s: &ConfigSaved, m: Moment, pace: Option<ConfigPace>) -> Markup {
    let first = s.changes.first();
    html! {
        div class="story" style="margin-top:16px" {
            div class="ev" {
                div class="tm" { "now" } div class="dot" { i {} }
                div class="tx" { b { "Saved." } " " (first.map_or("", |c| c.name.as_str())) " is "
                    (first.and_then(|c| c.now.as_ref()).map_or("not set", |n| n.value.as_str())) " on the topic." }
            }
            div class="ev idle" {
                div class="tm" { (when_label(m, pace)) } div class="dot" { i {} }
                div class="tx" { (reader(m)) }
            }
        }
    }
}

fn effect(s: &ConfigSaved) -> Markup {
    let first = s.changes.first().map_or("", |c| c.name.as_str());
    let Some(m) = moment(first) else {
        return html! {};
    };
    card(html! {
        h2 { "What it does to this log" }
        p class="sent" { (sentence(m, s.pace)) }
        p class="sent" style="margin-top:12px" {
            "A change does not rewrite anything. Each kind of config is read at its own moment."
        }
        (story(s, m, s.pace))
    })
}

/// The page.
#[must_use]
pub fn config_saved(s: &ConfigSaved, base: &Base) -> Markup {
    let back = link(
        &config_url(base, &s.head.name, ""),
        Variant::Plain,
        Size::Normal,
        None,
        "Back to the config",
    );
    html! {
        (page_head(&s.head, &sentence_of(s), back))
        (config_tabs(&s.head.name, base))
        div class="banner" {
            (glyph(Glyph::Check, 16, ""))
            div { b { "Saved." } " IncrementalAlterConfigs answered " span class="mono" { "0" } " for " span class="mono" { (s.head.name) } "." }
        }
        div class="g2" { (before_after(&s.changes)) (effect(s)) }
    }
}
