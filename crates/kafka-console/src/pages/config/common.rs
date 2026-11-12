//! What the Config pages of a topic share: the title row, the sentence under it, the tabs with Config showing, and the
//! words for values and sources.

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::config_words::{gloss, row_gloss};
use crate::model::{Cleanup, ConfigHead, ConfigRow, ConfigSource, Span};
use crate::ui::button::{State, pill_mono, state};
use crate::ui::kafka::{Tab, tabs, title_row};
use crate::ui::write::spans;

/// `/topics/orders/config{tail}` under the base.
#[must_use]
pub fn config_url(base: &Base, topic: &str, tail: &str) -> String {
    base.url(&format!("/topics/{}/config{tail}", segment(topic)))
}

/// `/topics/orders/config/edit?key=...`: the dialog for one config.
#[must_use]
pub fn edit_url(base: &Base, topic: &str, key: &str) -> String {
    config_url(base, topic, &format!("/edit?key={}", segment(key)))
}

/// `/topics/orders/config/reset?key=...`.
#[must_use]
pub fn reset_url(base: &Base, topic: &str, key: &str) -> String {
    config_url(base, topic, &format!("/reset?key={}", segment(key)))
}

/// The word beside the topic's name: a compacted topic says so, any other shows its policy.
#[must_use]
pub(super) fn policy_words(c: Cleanup) -> Markup {
    match c {
        Cleanup::Compact => state(State::Ok, "Compacted"),
        Cleanup::Both => pill_mono("compact,delete"),
        Cleanup::Delete => pill_mono("delete"),
    }
}

/// The head of a Config page: the name with its word, the sentence under it, and the buttons.
#[must_use]
pub(super) fn page_head(h: &ConfigHead, sentence: &[Span], acts: Markup) -> Markup {
    html! {
        div class="page-h" {
            div {
                (title_row(&h.name, policy_words(h.cleanup)))
                p class="sub" data-extra="live-sentence" { (spans(sentence)) }
            }
            div class="acts" { (acts) }
        }
    }
}

/// The tabs of a topic, Config showing, for the result pages (saved, refused) that carry their own head.
#[must_use]
pub(super) fn config_tabs(topic: &str, base: &Base) -> Markup {
    let t = base.url(&format!("/topics/{}", segment(topic)));
    let tab = |label, href: String, on| Tab {
        label,
        href,
        on,
        count: None,
    };
    tabs(&[
        tab("Messages", t.clone(), false),
        tab("Log", format!("{t}/log"), false),
        tab("Config", format!("{t}/config"), true),
        tab("Consumers", format!("{t}/consumers"), false),
        tab("Producers", format!("{t}/producers"), false),
    ])
}

/// The tag of a source, in the colour the boards give it.
#[must_use]
pub(super) fn source_tag(s: ConfigSource) -> Markup {
    html! { span class={"tag" @if s == ConfigSource::Default { " dim" }} { (s.label()) } }
}

/// A value in the monospace face with the small line that says it in words under it.
#[must_use]
pub(super) fn value_cell(r: &ConfigRow) -> Markup {
    html! {
        td class="mono" {
            (r.value)
            @if let Some(g) = row_gloss(r) { span class="sub" { (g) } }
        }
    }
}

/// A value of the dialogs, with its gloss in brackets where there is one.
#[must_use]
pub(super) fn plain_value(key: &str, value: &str) -> Markup {
    html! {
        span class="mono" { (value) }
        @if let Some(g) = gloss(key, value) { span class="sub" { (g) } }
    }
}
