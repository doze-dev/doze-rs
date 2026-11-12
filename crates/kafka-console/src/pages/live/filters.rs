//! The Live page's filter controls: the apps, APIs and topics as MultiSelects (a row passes a kind when it matches any chosen
//! value, and the kinds combine with AND), the chosen values as removable tags under the toolbar, and what an empty tape says.

use maud::{Markup, html};

use super::parts::LIVE_FRAGMENT;
use crate::base::Base;
use crate::model::{Facet, LiveView, WireQuery};
use crate::ui::client;
use crate::ui::components::{Choice, MultiSelect, multiselect};
use crate::ui::glyph::Glyph;

/// One kind of filter: the field its values travel in, its words, and the options the tape holds.
struct Kind<'a> {
    field: &'static str,
    label: &'static str,
    placeholder: &'static str,
    noun: &'static str,
    values: &'a [String],
    options: &'a [Facet],
}

fn kinds(v: &LiveView) -> [Kind<'_>; 3] {
    let q = &v.query;
    [
        Kind {
            field: "client",
            label: "Apps",
            placeholder: "All apps",
            noun: "apps",
            values: &q.client,
            options: &v.facets.clients,
        },
        Kind {
            field: "topic",
            label: "Topics",
            placeholder: "All topics",
            noun: "topics",
            values: &q.topic,
            options: &v.facets.topics,
        },
        Kind {
            field: "api",
            label: "APIs",
            placeholder: "All APIs",
            noun: "APIs",
            values: &q.api,
            options: &v.facets.apis,
        },
    ]
}

/// The three MultiSelects of the toolbar. A value the tape no longer holds stays a choice, so the page never lies about what it filters by.
pub(super) fn controls(v: &LiveView) -> Markup {
    html! {
        @for k in kinds(v) {
            @let choices: Vec<Choice<'_>> = k.options.iter().map(|o| Choice::new(&o.value, &o.value).count(o.count)).collect();
            (multiselect(&MultiSelect {
                auto_submit: true,
                placeholder: k.placeholder,
                noun: k.noun,
                class: "dz-chip",
                ui: Some("filter"),
                ..MultiSelect::new(&format!("live-{}", k.field), k.field, k.label, &choices, k.values)
            }))
        }
    }
}

fn live_url(base: &Base, q: &WireQuery) -> String {
    format!("{}{}", base.url("/live"), q.to_query())
}

/// The chosen values as tags that take themselves off, and "Clear filters"; nothing when fewer than two are chosen (the button says it).
pub(super) fn picked(v: &LiveView, base: &Base) -> Markup {
    let ks = kinds(v);
    let total: usize = ks.iter().map(|k| k.values.len()).sum();
    html! {
        div class="dz-picked" id="live-picked" {
            @if total > 1 {
                span class="dz-picked-k" { "Showing" }
                @for k in &ks {
                    @for val in k.values {
                        (client::in_place_link(LIVE_FRAGMENT, "dz-tag", &live_url(base, &v.query.without(k.field, val)), html! {
                            span class="dz-picked-f" { (k.noun.trim_end_matches('s')) " " }
                            span class="dz-tag-t" { (val) }
                            span class="dz-tag-x" aria-label={"Remove " (val)} { "\u{d7}" }
                        }))
                    }
                }
                (client::in_place_link(LIVE_FRAGMENT, "dz-link", &live_url(base, &cleared(&v.query)), html! { "Clear filters" }))
            }
        }
    }
}

/// The tape with no filter, keeping the page's own state out of it.
pub(super) fn cleared(q: &WireQuery) -> WireQuery {
    WireQuery {
        limit: q.limit,
        ..WireQuery::default()
    }
}

/// What the person asked for, in words, one phrase per filter that is on.
fn phrases(q: &WireQuery) -> Vec<String> {
    let list =
        |what: &str, vs: &[String]| (!vs.is_empty()).then(|| format!("{what} {}", vs.join(" or ")));
    let mut out: Vec<String> = [
        list("app is", &q.client),
        list("API is", &q.api),
        list("topic is", &q.topic),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !q.group.is_empty() {
        out.push(format!("group is {}", q.group));
    }
    if !q.kind.is_empty() {
        out.push(format!("kind is {}", q.kind));
    }
    if !q.text.is_empty() {
        out.push(format!("text contains \u{201c}{}\u{201d}", q.text));
    }
    if q.refused_only {
        out.push("it was refused".to_owned());
    }
    out
}

/// An empty tape under filters: which ones are on, how far back the tape goes, and a way out.
pub(super) fn no_match(v: &LiveView, base: &Base) -> Markup {
    let ph = phrases(&v.query);
    html! {
        div class="empty" id="live-no-match" style="padding:40px 32px" {
            div class="ic" { (crate::ui::glyph::glyph(Glyph::Filter, 22, "")) }
            h3 { "No request matches these filters" }
            p style="margin:0 0 14px" {
                "Among the " (v.window.rows) " rows kept, none has all of: "
                @for (i, p) in ph.iter().enumerate() { @if i > 0 { "; " } b { (p) } }
                "."
            }
            (client::in_place_link(LIVE_FRAGMENT, "btn sm", &live_url(base, &cleared(&v.query)), html! { (crate::ui::glyph::glyph(Glyph::X, 14, "")) "Clear filters" }))
        }
    }
}
