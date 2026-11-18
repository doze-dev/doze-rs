//! The "larder" world of the Config boards: `inventory.levels`, a compacted topic with three configs of its own and one
//! from a flag, as the boards draw it. Nothing here is a broker: the live pages read the real one; these answer the
//! same questions from fixed data, for the `boards` example and for the tests of the pages.

use crate::config_calls;
use crate::config_pass::after_of;
use crate::gather::{
    ConfigFields, ConfigMissing, EditState, EditWorld, config_changes, config_checked,
    config_input, config_missing, config_shape, config_view,
};
use crate::model::{
    Cleanup, ConfigAsk, ConfigEditView, ConfigForm, ConfigHead, ConfigOp, ConfigPace, ConfigPage,
    ConfigPass, ConfigRefusal, ConfigRefused, ConfigResetResult, ConfigResetView, ConfigResult,
    ConfigRow, ConfigSaved, ConfigSource, ConfigSynonym, ConfigValue,
};
use crate::refusal::refusal;

/// The topic the boards draw.
pub const TOPIC: &str = "inventory.levels";
/// The broker's pace in the larder: the cleaner looks every 15 s, retention every 5 minutes.
pub const PACE: ConfigPace = ConfigPace {
    cleaner_secs: 15,
    cleaner_on: true,
    retention_secs: 300,
};
const VERSION: i16 = 1;

use ConfigSource::{Default as Dflt, DynamicTopic as Topic, StaticFlag as Flag};

mod specs;
use specs::{SPECS, Spec};

fn syn(name: &str, value: &str, source: ConfigSource) -> ConfigSynonym {
    ConfigSynonym {
        name: name.to_owned(),
        value: value.to_owned(),
        source,
    }
}

/// The places behind a config of the larder's topic (the broker names behind it, where it has one).
fn synonyms(key: &str, value: &str, source: ConfigSource) -> Vec<ConfigSynonym> {
    let own = (source == Topic).then(|| syn(key, value, Topic));
    let behind: Vec<ConfigSynonym> = match key {
        "cleanup.policy" => vec![syn("log.cleanup.policy", "delete", Dflt)],
        "segment.bytes" => vec![
            syn("log.segment.bytes", "134217728", Flag),
            syn("log.segment.bytes", "1073741824", Dflt),
        ],
        "retention.ms" => vec![
            syn("log.retention.ms", "-1", Flag),
            syn("log.retention.hours", "168", Dflt),
        ],
        "min.compaction.lag.ms" => vec![syn("log.cleaner.min.compaction.lag.ms", "0", Dflt)],
        "min.cleanable.dirty.ratio" => vec![syn("log.cleaner.min.cleanable.ratio", "0.5", Dflt)],
        _ => Vec::new(),
    };
    own.into_iter().chain(behind).collect()
}

fn row_of(spec: &Spec) -> ConfigRow {
    let (name, value, source, kind, doc, allowed) = *spec;
    ConfigRow {
        name: name.to_owned(),
        value: value.to_owned(),
        source,
        kind,
        doc: doc.to_owned(),
        allowed: allowed.to_owned(),
        synonyms: synonyms(name, value, source),
    }
}

/// The rows of a topic of the larder. `inventory.levels` is the board's; any other topic sets nothing of its own.
#[must_use]
pub fn rows(topic: &str) -> Vec<ConfigRow> {
    let mut rows: Vec<ConfigRow> = SPECS.iter().map(row_of).collect();
    if topic != TOPIC {
        for r in &mut rows {
            let (value, source) = match r.name.as_str() {
                "cleanup.policy" => ("delete", Dflt),
                "segment.bytes" => ("134217728", Flag),
                "min.compaction.lag.ms" => ("0", Dflt),
                "retention.ms" => ("604800000", Dflt),
                _ => continue,
            };
            r.value = value.to_owned();
            r.source = source;
            r.synonyms = synonyms(&r.name, value, source);
        }
    }
    rows
}

fn known(topic: &str) -> bool {
    topic == TOPIC || crate::fixtures_topics::head(topic).is_some()
}

fn head(topic: &str, rows: &[ConfigRow]) -> ConfigHead {
    let compact = rows
        .iter()
        .any(|r| r.name == "cleanup.policy" && r.value == "compact");
    ConfigHead {
        name: topic.to_owned(),
        cleanup: if compact {
            Cleanup::Compact
        } else {
            Cleanup::Delete
        },
    }
}

/// The page; `None` for a topic the larder does not have.
#[must_use]
pub fn page(topic: &str, ask: ConfigAsk) -> Option<ConfigPage> {
    known(topic).then(|| {
        let rows = rows(topic);
        ConfigPage {
            head: head(topic, &rows),
            rows,
            pace: Some(PACE),
            ask,
        }
    })
}

/// The edit dialog.
///
/// # Errors
/// [`ConfigMissing`] for a topic or a config the larder does not have.
pub fn form(topic: &str, f: ConfigFields<'_>) -> Result<ConfigEditView, ConfigMissing> {
    if !known(topic) {
        return Err(ConfigMissing::Topic);
    }
    let all = rows(topic);
    let s = config_shape(&all, f.key).ok_or(ConfigMissing::Key)?;
    let input = config_input(&f, s.0, s.1, s.2);
    let w = EditWorld {
        topic,
        rows: &all,
        pace: Some(PACE),
        version: VERSION,
    };
    Ok(config_view(&w, s, input, EditState::default()))
}

fn value_of(rows: &[ConfigRow], key: &str) -> Option<ConfigValue> {
    rows.iter().find(|r| r.name == key).map(|r| ConfigValue {
        value: r.value.clone(),
        source: r.source,
    })
}

/// What the larder's broker would say to a request: the code and message of Kafka's own wording for the two cases the
/// boards draw (the lags crossed, a policy that is not one), and for a value that is not a number.
fn verdict(changes: &[config_calls::Change]) -> Option<(i16, String)> {
    let get = |k: &str| changes.iter().find(|c| c.0 == k).and_then(|c| c.2.clone());
    if let (Some(min), Some(max)) = (get("min.compaction.lag.ms"), get("max.compaction.lag.ms"))
        && let (Ok(a), Ok(b)) = (min.parse::<i64>(), max.parse::<i64>())
        && a > b
    {
        return Some((
            40,
            format!(
                "conflict topic config setting min.compaction.lag.ms ({a}) > max.compaction.lag.ms ({b})"
            ),
        ));
    }
    changes.iter().find_map(|(k, op, v)| {
        let v = v.as_deref()?;
        (k == "cleanup.policy" && *op != ConfigOp::Subtract && v.split(',').any(|p| !matches!(p.trim(), "compact" | "delete")))
            .then(|| (40, format!("Invalid value {v} for configuration cleanup.policy: String must be one of: compact, delete")))
    })
}

fn refused_of(
    topic: &str,
    all: &[ConfigRow],
    form: ConfigForm,
    input: &crate::model::ConfigInput,
    said: (i16, &str),
) -> ConfigRefused {
    let lags = form == ConfigForm::Lags;
    let swapped = |k: &str, v: &str| (k.to_owned(), v.to_owned());
    let pass = if lags {
        let pairs = vec![
            swapped("min.compaction.lag.ms", &input.other),
            swapped("max.compaction.lag.ms", &input.value),
        ];
        vec![ConfigPass {
            lines: vec![
                format!("min.compaction.lag.ms  SET  {}", input.other),
                format!("max.compaction.lag.ms  SET  {}", input.value),
            ],
            result: None,
            pairs,
        }]
    } else {
        let current = all
            .iter()
            .find(|r| r.name == input.key)
            .map_or("", |r| r.value.as_str());
        vec![ConfigPass {
            lines: vec![format!("{}  {}  delete", input.key, input.op.word())],
            result: Some(after_of(current, input.op, "delete")),
            pairs: vec![(input.key.clone(), "delete".to_owned())],
        }]
    };
    ConfigRefused {
        head: head(topic, all),
        kind: if lags {
            ConfigRefusal::Lags
        } else {
            ConfigRefusal::Value
        },
        input: input.clone(),
        refusal: refusal(said.0, Some(said.1)),
        request: config_calls::alter(VERSION, topic, &config_changes(form, input)),
        pass,
        others: if lags { Vec::new() } else { others() },
    }
}

fn others() -> Vec<(String, String)> {
    [
        ("segment.bytes 512", "Invalid value 512 for configuration segment.bytes: Value must be at least 1024"),
        ("min.cleanable.dirty.ratio 1.5", "Invalid value 1.5 for configuration min.cleanable.dirty.ratio: Value must be no more than 1"),
        ("segment.bytes abc", "Invalid value abc for configuration segment.bytes: Not a number of type INT"),
    ]
    .iter()
    .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
    .collect()
}

/// A save, or with `check` a dry run.
#[must_use]
pub fn save(topic: &str, f: ConfigFields<'_>, check: bool) -> ConfigResult {
    if !known(topic) {
        return ConfigResult::NoTopic;
    }
    let before = rows(topic);
    let Some(s) = config_shape(&before, f.key) else {
        return ConfigResult::NoKey;
    };
    let input = config_input(&f, s.0, s.1, s.2);
    let w = EditWorld {
        topic,
        rows: &before,
        pace: Some(PACE),
        version: VERSION,
    };
    if let Some(e) = config_missing(s.0, &input) {
        return ConfigResult::Invalid(Box::new(config_view(
            &w,
            s,
            input,
            EditState {
                error: Some(e),
                checked: None,
            },
        )));
    }
    let changes = config_changes(s.0, &input);
    if let Some((code, said)) = verdict(&changes) {
        return ConfigResult::Refused(Box::new(refused_of(
            topic,
            &before,
            s.0,
            &input,
            (code, &said),
        )));
    }
    if check {
        let state = EditState {
            error: None,
            checked: Some(config_checked(&before, &changes)),
        };
        return ConfigResult::Checked(Box::new(config_view(&w, s, input, state)));
    }
    let mut now = before.clone();
    for (key, op, value) in &changes {
        if let Some(r) = now.iter_mut().find(|r| &r.name == key) {
            r.value = if *op == ConfigOp::Set {
                value.clone().unwrap_or_default()
            } else {
                after_of(&r.value, *op, value.as_deref().unwrap_or(""))
            };
            r.source = Topic;
        }
    }
    ConfigResult::Saved(Box::new(ConfigSaved {
        head: head(topic, &now),
        changes: changes
            .iter()
            .map(|(k, op, _)| crate::model::ConfigChange {
                name: k.clone(),
                op: *op,
                before: value_of(&before, k),
                now: value_of(&now, k),
            })
            .collect(),
        total: now.len(),
        pace: Some(PACE),
    }))
}

/// The reset dialog.
///
/// # Errors
/// [`ConfigMissing`] for a topic or a config the larder does not have.
pub fn reset_form(topic: &str, key: &str) -> Result<ConfigResetView, ConfigMissing> {
    if !known(topic) {
        return Err(ConfigMissing::Topic);
    }
    let row = rows(topic)
        .into_iter()
        .find(|r| r.name == key)
        .ok_or(ConfigMissing::Key)?;
    let after = row.fallback().map(|s| ConfigValue {
        value: s.value.clone(),
        source: s.source,
    });
    let call = config_calls::alter(
        VERSION,
        topic,
        &[(row.name.clone(), ConfigOp::Delete, None)],
    );
    Ok(ConfigResetView {
        topic: topic.to_owned(),
        row,
        after,
        pace: Some(PACE),
        call,
        refusal: None,
    })
}

/// A reset: the larder always takes it.
#[must_use]
pub fn reset(topic: &str, key: &str) -> ConfigResetResult {
    match reset_form(topic, key) {
        Ok(_) => ConfigResetResult::Done,
        Err(ConfigMissing::Topic) => ConfigResetResult::NoTopic,
        Err(ConfigMissing::Key) => ConfigResetResult::NoKey,
    }
}
