//! The two refusals (boards `config-refused`: the lags together; `config-refused-value`: a value Kafka would not take).
//! The code and the sentence are the broker's own. "What would pass" is a request the broker itself accepted when asked
//! with `validate_only`; the other ranges are what it answered to those values just now.

use maud::{Markup, html};

use super::common::{config_tabs, config_url, edit_url, page_head};
use crate::base::{Base, segment};
use crate::config_teach::{MAX_LAG, MIN_LAG};
use crate::config_words::gloss;
use crate::model::{ConfigPass, ConfigRefusal, ConfigRefused, Span};
use crate::ui::button::{Size, Variant, link};
use crate::ui::code::block;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::layout::{card, hr};

fn sentence(r: &ConfigRefused) -> Vec<Span> {
    let said = match (r.kind, r.refusal.code) {
        (ConfigRefusal::Lags, _) => {
            "Kafka checks the two lags together and found the minimum above the maximum.".to_owned()
        }
        (_, 40) => format!("Kafka refused the value you sent for {}.", r.input.key),
        _ => "Kafka refused the request.".to_owned(),
    };
    vec![Span::text(format!("Nothing was changed. {said}"))]
}

fn callout(r: &ConfigRefused) -> Markup {
    html! {
        div class="callout refused" data-extra="refusal" style="margin-bottom:24px" {
            (glyph(Glyph::Warn, 18, ""))
            div {
                span class="st refused" { "Refused" } " " span class="mono" { (r.refusal.name) " (" (r.refusal.code) ")" }
                br; b { (r.refusal.message) }
            }
        }
    }
}

/// The address of the dialog again, with the values to start from.
fn again(r: &ConfigRefused, base: &Base, topic: &str, pairs: &[(String, String)]) -> String {
    let mut url = edit_url(base, topic, &r.input.key);
    for (key, value) in pairs {
        let name = match key.as_str() {
            MAX_LAG => "other",
            _ => "value",
        };
        url.push_str(&format!("&{name}={}", segment(value)));
    }
    if r.input.op != crate::model::ConfigOp::Set {
        url.push_str(&format!("&op={}", r.input.op.form()));
    }
    url
}

fn sent(r: &ConfigRefused) -> Markup {
    block(&r.request, None, Some("margin-top:12px"))
}

fn lag_pass(p: &ConfigPass) -> Markup {
    html! {
        dl class="kv sm" {
            @for (key, value) in &p.pairs {
                dt { @if key == MIN_LAG { "min" } @else if key == MAX_LAG { "max" } @else { (key) } }
                dd class="mono" { (value) @if let Some(g) = gloss(key, value) { span class="sub" { (g) } } }
            }
        }
    }
}

fn lags_page(r: &ConfigRefused, base: &Base) -> Markup {
    let topic = &r.head.name;
    let pass = r.pass.first();
    let pairs: Vec<(String, String)> = pass.map_or_else(
        || {
            vec![
                (MIN_LAG.to_owned(), r.input.value.clone()),
                (MAX_LAG.to_owned(), r.input.other.clone()),
            ]
        },
        |p| p.pairs.clone(),
    );
    html! {
        div class="g2" {
            (card(html! {
                h2 { "What you sent" } (sent(r))
                p class="help" style="margin-top:12px" {
                    "Both keys were in one request, so both were refused. Other resources in the same request are answered on their own."
                }
            }))
            (card(html! {
                h2 { "What would pass" }
                @if let Some(p) = pass {
                    p class="lede" { "min is how long a record waits at least, max how long at most: min cannot be above max. Kafka took these when asked with validate_only." }
                    (lag_pass(p))
                } @else {
                    p class="lede" { "min is how long a record waits at least, max how long at most: min cannot be above max." }
                }
                div class="row" style="margin-top:16px" { (link(&again(r, base, topic, &pairs), Variant::Plain, Size::Normal, None, "Edit again")) }
            }))
        }
    }
}

fn pass_block(p: &ConfigPass) -> Markup {
    html! {
        pre class="code" style="margin-top:12px" {
            (p.lines.join("\n"))
            @if let Some(result) = &p.result { "\n  -> " (result) }
        }
    }
}

fn others(r: &ConfigRefused) -> Markup {
    card(html! {
        h2 { "The same words for other ranges" }
        p class="lede" { "Kafka names the config, the value, and the rule it broke. These are its answers to those values now." }
        table class="t" style="margin-top:12px" {
            thead { tr { th scope="col" { "You send" } th scope="col" { "Kafka says" } } }
            tbody { @for (send, said) in &r.others { tr { td class="mono" { (send) } td class="mono" { (said) } } } }
        }
    })
}

fn value_page(r: &ConfigRefused, base: &Base) -> Markup {
    let topic = &r.head.name;
    let pairs = r.pass.first().map(|p| p.pairs.clone()).unwrap_or_default();
    html! {
        div class="g2" {
            (card(html! {
                h2 { "What you sent" } (sent(r))
                @if !r.pass.is_empty() {
                    (hr())
                    h2 { "What would pass" }
                    @for p in &r.pass { (pass_block(p)) }
                }
                div class="row" style="margin-top:16px" { (link(&again(r, base, topic, &pairs), Variant::Plain, Size::Normal, None, "Edit again")) }
            }))
            @if !r.others.is_empty() { (others(r)) }
        }
    }
}

/// The page, in the layout of the kind of refusal it is.
#[must_use]
pub fn config_refused(r: &ConfigRefused, base: &Base) -> Markup {
    let back = link(
        &config_url(base, &r.head.name, ""),
        Variant::Plain,
        Size::Normal,
        None,
        "Back to the config",
    );
    html! {
        (page_head(&r.head, &sentence(r), back))
        (config_tabs(&r.head.name, base))
        (callout(r))
        @match r.kind {
            ConfigRefusal::Lags => (lags_page(r, base)),
            ConfigRefusal::Value => (value_page(r, base)),
        }
    }
}
