//! The edit dialogs of the Config page and what a save comes to: IncrementalAlterConfigs (SET, APPEND, SUBTRACT, or with
//! `validate_only` a dry run), then DescribeConfigs again for what the config is now.

use kafka_wire::api::Response;
use kafka_wire::messages::ApiKey;

use super::config::{ConfigMissing, head, pace, rows};
use super::config_refuse;
use crate::backend::{Backend, requests::alter_configs};
use crate::config_calls::{self, Change};
use crate::config_pass::after_of;
use crate::config_teach::{MAX_LAG, MIN_LAG, teach};
use crate::model::{
    ConfigChange, ConfigChecked, ConfigEditView, ConfigForm, ConfigInput, ConfigOp, ConfigPace,
    ConfigResult, ConfigRow, ConfigSaved, ConfigValue,
};

/// What a form carried. A field the form did not carry is `None` and takes the dialog's default.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfigFields<'a> {
    pub key: &'a str,
    pub op: Option<&'a str>,
    pub value: Option<&'a str>,
    pub other: Option<&'a str>,
}

/// The dialog a config is edited in, with the row (for the lags, the minimum's) and the second lag's row.
pub fn shape<'a>(
    rows: &'a [ConfigRow],
    key: &str,
) -> Option<(ConfigForm, &'a ConfigRow, Option<&'a ConfigRow>)> {
    let find = |name: &str| rows.iter().find(|r| r.name == name);
    if key == MIN_LAG || key == MAX_LAG {
        let (min, max) = (find(MIN_LAG)?, find(MAX_LAG)?);
        return Some((ConfigForm::Lags, min, Some(max)));
    }
    let row = find(key)?;
    let form = if row.is_list() {
        ConfigForm::List
    } else {
        ConfigForm::Single
    };
    Some((form, row, None))
}

#[must_use]
pub fn input_of(
    f: &ConfigFields<'_>,
    form: ConfigForm,
    row: &ConfigRow,
    other: Option<&ConfigRow>,
) -> ConfigInput {
    let op = match form {
        ConfigForm::List => match f.op.map(ConfigOp::from_form) {
            Some(ConfigOp::Delete) | None => ConfigOp::Append,
            Some(op) => op,
        },
        _ => ConfigOp::Set,
    };
    let own = if form == ConfigForm::List {
        ""
    } else {
        row.value.as_str()
    };
    ConfigInput {
        key: row.name.clone(),
        op,
        value: f.value.unwrap_or(own).trim().to_owned(),
        other: f
            .other
            .or_else(|| other.map(|o| o.value.as_str()))
            .unwrap_or("")
            .trim()
            .to_owned(),
    }
}

/// The changes the dialog sends.
pub fn changes_of(form: ConfigForm, i: &ConfigInput) -> Vec<Change> {
    match form {
        ConfigForm::Lags => vec![
            (MIN_LAG.to_owned(), ConfigOp::Set, Some(i.value.clone())),
            (MAX_LAG.to_owned(), ConfigOp::Set, Some(i.other.clone())),
        ],
        _ => vec![(i.key.clone(), i.op, Some(i.value.clone()))],
    }
}

/// What is missing from a form, in words; a delete has nothing to type and the dialogs have none.
#[must_use]
pub fn missing(form: ConfigForm, i: &ConfigInput) -> Option<String> {
    match form {
        ConfigForm::Lags if i.value.is_empty() || i.other.is_empty() => {
            Some("Type both lags.".to_owned())
        }
        ConfigForm::List if i.value.is_empty() => Some("Type the policy to use.".to_owned()),
        ConfigForm::Single if i.value.is_empty() => Some("Type a value.".to_owned()),
        _ => None,
    }
}

fn retention_of(rows: &[ConfigRow]) -> Option<i64> {
    rows.iter()
        .find(|r| r.name == "retention.ms")
        .and_then(|r| r.value.parse().ok())
}

/// The dialog and the rows it is made from.
#[derive(Debug, Clone, Copy)]
pub struct EditWorld<'a> {
    pub topic: &'a str,
    pub rows: &'a [ConfigRow],
    pub pace: Option<ConfigPace>,
    /// The version of IncrementalAlterConfigs the call is drawn at.
    pub version: i16,
}

/// The dialog to build: the form, the row (for the lags, the minimum's), and the second lag's row.
type Shape<'a> = (ConfigForm, &'a ConfigRow, Option<&'a ConfigRow>);

/// What the dialog says besides the form: a field error, and a dry run's answer.
#[derive(Debug, Clone, Default)]
pub struct EditState {
    pub error: Option<String>,
    pub checked: Option<ConfigChecked>,
}

/// The dialog for the typed input (pure: everything it says is in the arguments).
#[must_use]
pub fn view_of(
    w: &EditWorld<'_>,
    shape: Shape<'_>,
    input: ConfigInput,
    state: EditState,
) -> ConfigEditView {
    let (form, row, other) = shape;
    ConfigEditView {
        topic: w.topic.to_owned(),
        form,
        teach: teach(row, other, form, &input, w.pace, retention_of(w.rows)),
        call: config_calls::alter(w.version, w.topic, &changes_of(form, &input)),
        row: row.clone(),
        other: other.cloned(),
        input,
        pace: w.pace,
        error: state.error,
        checked: state.checked,
    }
}

fn world<'a>(b: &Backend, topic: &'a str, rows: &'a [ConfigRow]) -> EditWorld<'a> {
    EditWorld {
        topic,
        rows,
        pace: pace(b),
        version: b.version(ApiKey::IncrementalAlterConfigs),
    }
}

/// The dialog for a config of a topic.
///
/// # Errors
/// [`ConfigMissing`] if the topic or the config is not there.
pub async fn form(
    b: &Backend,
    topic: &str,
    f: ConfigFields<'_>,
) -> Result<ConfigEditView, ConfigMissing> {
    let all = rows(b, topic).await.ok_or(ConfigMissing::Topic)?;
    let s = shape(&all, f.key).ok_or(ConfigMissing::Key)?;
    let input = input_of(&f, s.0, s.1, s.2);
    Ok(view_of(
        &world(b, topic, &all),
        s,
        input,
        EditState::default(),
    ))
}

/// What the broker answered one resource of IncrementalAlterConfigs: its code and its message.
pub(super) async fn send(
    b: &Backend,
    topic: &str,
    changes: &[Change],
    validate_only: bool,
) -> Option<(i16, Option<String>)> {
    let list: Vec<(&str, ConfigOp, Option<&str>)> = changes
        .iter()
        .map(|(k, op, v)| (k.as_str(), *op, v.as_deref()))
        .collect();
    let Ok(Response::IncrementalAlterConfigs(r)) = b
        .call(alter_configs::alter(topic, &list, validate_only))
        .await
    else {
        return None;
    };
    let a = r.responses.into_iter().next()?;
    Some((a.error_code, a.error_message.map(|m| m.to_string())))
}

fn value_of(rows: &[ConfigRow], key: &str) -> Option<ConfigValue> {
    rows.iter().find(|r| r.name == key).map(|r| ConfigValue {
        value: r.value.clone(),
        source: r.source,
    })
}

#[must_use]
pub fn checked_of(rows: &[ConfigRow], changes: &[Change]) -> ConfigChecked {
    ConfigChecked {
        after: changes
            .iter()
            .map(|(k, op, v)| {
                let now = rows
                    .iter()
                    .find(|r| &r.name == k)
                    .map(|r| r.value.clone())
                    .unwrap_or_default();
                let typed = v.clone().unwrap_or_default();
                let after = if *op == ConfigOp::Set {
                    typed
                } else {
                    after_of(&now, *op, &typed)
                };
                (k.clone(), now, after)
            })
            .collect(),
    }
}

/// Saves the form (or, with `check`, asks the broker whether it would take it and changes nothing).
pub async fn save(b: &Backend, topic: &str, f: ConfigFields<'_>, check: bool) -> ConfigResult {
    let Some(before) = rows(b, topic).await else {
        return ConfigResult::NoTopic;
    };
    let Some(s) = shape(&before, f.key) else {
        return ConfigResult::NoKey;
    };
    let input = input_of(&f, s.0, s.1, s.2);
    if let Some(e) = missing(s.0, &input) {
        let state = EditState {
            error: Some(e),
            checked: None,
        };
        return ConfigResult::Invalid(Box::new(view_of(
            &world(b, topic, &before),
            s,
            input,
            state,
        )));
    }
    let changes = changes_of(s.0, &input);
    let Some((code, message)) = send(b, topic, &changes, check).await else {
        return ConfigResult::NoTopic;
    };
    if code != 0 {
        let said = config_refuse::Said {
            code,
            message: message.as_deref(),
        };
        let r = config_refuse::refused(b, topic, &before, (s.0, &input), said).await;
        return ConfigResult::Refused(Box::new(r));
    }
    if check {
        let state = EditState {
            error: None,
            checked: Some(checked_of(&before, &changes)),
        };
        return ConfigResult::Checked(Box::new(view_of(
            &world(b, topic, &before),
            s,
            input,
            state,
        )));
    }
    let Some(now) = rows(b, topic).await else {
        return ConfigResult::NoTopic;
    };
    ConfigResult::Saved(Box::new(ConfigSaved {
        head: head(topic, &now),
        changes: changes
            .iter()
            .map(|(k, op, _)| ConfigChange {
                name: k.clone(),
                op: *op,
                before: value_of(&before, k),
                now: value_of(&now, k),
            })
            .collect(),
        total: now.len(),
        pace: pace(b),
    }))
}
