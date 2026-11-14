//! A refused save: Kafka's code and words, then what the broker itself would take. A request is shown as "what would
//! pass" only after the broker accepted it with `validate_only`; the other ranges are answers the broker really gave to
//! those values now, never a copy of its rules.

use kafka_wire::messages::ApiKey;

use super::config::head;
use super::config_edit::send;
use crate::backend::Backend;
use crate::config_calls::{self, Change};
use crate::config_pass::{after_of, options};
use crate::config_teach::{MAX_LAG, MIN_LAG};
use crate::model::{
    ConfigForm, ConfigInput, ConfigOp, ConfigPass, ConfigRefusal, ConfigRefused, ConfigRow,
};
use crate::refusal::refusal;

/// What the broker said of the save.
#[derive(Debug, Clone, Copy)]
pub(super) struct Said<'a> {
    pub code: i16,
    pub message: Option<&'a str>,
}

/// Other values and what Kafka says to each, asked of the broker whenever a value is refused.
const SAMPLES: [(&str, &str); 3] = [
    ("segment.bytes", "512"),
    ("min.cleanable.dirty.ratio", "1.5"),
    ("segment.bytes", "abc"),
];

/// Whether the broker would take these changes.
async fn passes(b: &Backend, topic: &str, changes: &[Change]) -> bool {
    matches!(send(b, topic, changes, true).await, Some((0, _)))
}

fn pass_of(changes: &[Change], result: Option<String>) -> ConfigPass {
    ConfigPass {
        lines: config_calls::change_text(changes),
        result,
        pairs: changes
            .iter()
            .map(|(k, _, v)| (k.clone(), v.clone().unwrap_or_default()))
            .collect(),
    }
}

/// The two lags with their values swapped, if the broker takes that.
async fn swapped(b: &Backend, topic: &str, i: &ConfigInput) -> Vec<ConfigPass> {
    let changes = vec![
        (MIN_LAG.to_owned(), ConfigOp::Set, Some(i.other.clone())),
        (MAX_LAG.to_owned(), ConfigOp::Set, Some(i.value.clone())),
    ];
    if passes(b, topic, &changes).await {
        vec![pass_of(&changes, None)]
    } else {
        Vec::new()
    }
}

/// The values the refusal names that the broker takes in place of the one sent.
async fn named(
    b: &Backend,
    topic: &str,
    i: &ConfigInput,
    message: &str,
    current: &str,
) -> Vec<ConfigPass> {
    let mut out = Vec::new();
    for option in options(message) {
        let changes = vec![(i.key.clone(), i.op, Some(option.clone()))];
        if passes(b, topic, &changes).await {
            let result = (i.op != ConfigOp::Set).then(|| after_of(current, i.op, &option));
            // An element already in the list passes and changes nothing: not worth offering.
            if result.as_deref() != Some(current) {
                out.push(pass_of(&changes, result));
            }
        }
    }
    out
}

/// What Kafka says to the sample values the broker refuses, other than the one just sent.
async fn others(b: &Backend, topic: &str, i: &ConfigInput) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (key, value) in SAMPLES {
        if i.key == key && i.value == value {
            continue;
        }
        let change = vec![(key.to_owned(), ConfigOp::Set, Some(value.to_owned()))];
        if let Some((code, Some(said))) = send(b, topic, &change, true).await
            && code != 0
        {
            out.push((format!("{key} {value}"), said));
        }
    }
    out
}

/// The refused state for a save the broker refused.
pub(super) async fn refused(
    b: &Backend,
    topic: &str,
    before: &[ConfigRow],
    (form, input): (ConfigForm, &ConfigInput),
    said: Said<'_>,
) -> ConfigRefused {
    let message = said.message.unwrap_or("");
    let conflict = form == ConfigForm::Lags && message.starts_with("conflict topic config");
    let current = before
        .iter()
        .find(|r| r.name == input.key)
        .map_or("", |r| r.value.as_str());
    let (kind, pass) = if conflict {
        (ConfigRefusal::Lags, swapped(b, topic, input).await)
    } else if said.code == 40 && form != ConfigForm::Lags {
        (
            ConfigRefusal::Value,
            named(b, topic, input, message, current).await,
        )
    } else {
        (ConfigRefusal::Value, Vec::new())
    };
    let others = if said.code == 40 {
        others(b, topic, input).await
    } else {
        Vec::new()
    };
    ConfigRefused {
        head: head(topic, before),
        kind,
        input: input.clone(),
        refusal: refusal(said.code, said.message),
        request: config_calls::alter(
            b.version(ApiKey::IncrementalAlterConfigs),
            topic,
            &super::config_edit::changes_of(form, input),
        ),
        pass,
        others,
    }
}
