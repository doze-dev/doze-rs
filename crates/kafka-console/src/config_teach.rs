//! The five lines under the field of an edit dialog: what the config is, what it is now, what it may be, when a change
//! counts, and what is so here. Pure functions from the rows the broker described and what was typed to [`ConfigTeach`];
//! nothing is said that a row or the broker's own table did not say.

use crate::config_effects::{moment, takes_effect};
use crate::config_words::{gloss, policy_gloss};
use crate::format::span_ms;
use crate::model::{
    ConfigForm, ConfigInput, ConfigPace, ConfigRow, ConfigSource, ConfigTeach, Span,
};
use crate::teach::plain_doc;

/// The lags of the dialog that edits both.
pub const MIN_LAG: &str = "min.compaction.lag.ms";
pub const MAX_LAG: &str = "max.compaction.lag.ms";

fn t(s: &str) -> Span {
    Span::text(s)
}

fn m(s: impl Into<String>) -> Span {
    Span::mono(s)
}

fn where_from(r: &ConfigRow) -> &'static str {
    match r.source {
        ConfigSource::DynamicTopic => "set on this topic",
        ConfigSource::StaticFlag => "from a broker flag; nothing is set on this topic",
        ConfigSource::DynamicBroker | ConfigSource::DynamicDefault => {
            "set for the broker; nothing is set on this topic"
        }
        _ => "Kafka's default; nothing is set on this topic",
    }
}

fn now_of(r: &ConfigRow) -> Vec<Span> {
    let mut out = vec![m(r.value.clone()), t(", "), t(where_from(r))];
    if let Some(g) = gloss(&r.name, &r.value) {
        out.push(t(&format!(" ({g})")));
    }
    out
}

fn lags_now(min: &ConfigRow, max: &ConfigRow) -> Vec<Span> {
    let side = |label: &str, r: &ConfigRow| {
        let mut v = vec![t(label), t(" "), m(r.value.clone())];
        if r.on_topic() {
            v.push(t(" set on this topic"));
        } else {
            v.push(t(" not set"));
            if let Some(g) = gloss(&r.name, &r.value) {
                v.push(t(&format!(" ({g})")));
            }
        }
        v
    };
    let mut out = side("min", min);
    out.push(t("; "));
    out.extend(side("max", max));
    out
}

fn lags_here(input: &ConfigInput) -> Vec<Span> {
    let (Ok(min), Ok(max)) = (
        input.value.trim().parse::<i64>(),
        input.other.trim().parse::<i64>(),
    ) else {
        return Vec::new();
    };
    if min < 0 || max < 1 || min > max {
        return Vec::new();
    }
    let long = |ms: i64| {
        if ms >= 1000 {
            span_ms(ms)
        } else {
            format!("{ms} ms")
        }
    };
    vec![t(&format!(
        "with min {} a record is left alone for {}; with max {} it would be forced after {}",
        long(min),
        long(min),
        long(max),
        long(max)
    ))]
}

fn policy_here(r: &ConfigRow, retention_ms: Option<i64>) -> Vec<Span> {
    let kept = match retention_ms {
        Some(ms) if ms >= 1000 => format!("retention ({}) trims this topic too", span_ms(ms)),
        Some(ms) if ms < 0 => {
            "retention keeps records for ever, so nothing is trimmed by age".to_owned()
        }
        _ => "retention trims this topic too".to_owned(),
    };
    match policy_gloss(&r.value) {
        Some(now) => vec![
            t("now it does this: "),
            t(now),
            t(". Adding "),
            m("delete"),
            t(&format!(" makes {kept}, and allows Delete records")),
        ],
        None => Vec::new(),
    }
}

/// What the dialog for `row` teaches. `other` is the second lag in the dialog that edits both; `retention_ms` is the
/// topic's `retention.ms` where the page read it.
#[must_use]
pub fn teach(
    row: &ConfigRow,
    other: Option<&ConfigRow>,
    form: ConfigForm,
    input: &ConfigInput,
    pace: Option<ConfigPace>,
    retention_ms: Option<i64>,
) -> ConfigTeach {
    let what = crate::config_effects::what_it_is(&row.name)
        .map(str::to_owned)
        .unwrap_or_else(|| plain_doc(&row.doc));
    let takes = moment(&row.name).map_or_else(
        || "when the broker next reads it".to_owned(),
        |mo| takes_effect(mo, pace),
    );
    let (now, allowed, here) = match (form, other) {
        (ConfigForm::Lags, Some(max)) => (
            lags_now(row, max),
            vec![t(&format!(
                "min {}, max {}, and min not above max (Kafka checks the two together)",
                row.allowed, max.allowed
            ))],
            lags_here(input),
        ),
        _ => (
            now_of(row),
            vec![t(if row.allowed.is_empty() {
                "what Kafka's table says for it"
            } else {
                &row.allowed
            })],
            if row.name == "cleanup.policy" {
                policy_here(row, retention_ms)
            } else {
                Vec::new()
            },
        ),
    };
    ConfigTeach {
        what: vec![t(&what)],
        now,
        allowed,
        takes_effect: vec![t(&takes)],
        here,
    }
}

#[cfg(test)]
mod tests;
