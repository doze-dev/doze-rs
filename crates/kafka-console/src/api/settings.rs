//! The Settings page's API: the page (`GET /api/v1/settings`), the edit dialog of one of the five dynamic broker configs
//! (`GET /api/v1/settings/edit?key=&scope=`) and its write (`POST /api/v1/settings/edit`), which checks (`validate_only`) or
//! saves through the classic console's own path (`Source::settings_save`: IncrementalAlterConfigs as the console's principal),
//! answering in Kafka's words. No memory figure is ever sent (the weight carries the binary's size and the open partitions).
//!
//! Also the two serialisers the view-models of Settings and Access use for types they share with other pages.

use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize, Serializer};

use super::{error, json};
use crate::gather::settings_edit::SettingsFields;
use crate::model::{
    ConfigSource, Refusal, SettingsEditView, SettingsRefused, SettingsResult, SettingsSaved,
    SettingsView, Span,
};
use crate::state::Shared;

/// A refusal as the pages show it: Kafka's error name and number, its sentence, and the console's reading where it has one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct KafkaRefusal {
    /// `36`.
    pub code: i16,
    /// `INVALID_CONFIG`.
    pub name: String,
    /// The broker's own sentence for this call, else Kafka's description of the code.
    pub message: String,
    pub rule: Option<Vec<Span>>,
    pub would_pass: Option<Vec<Span>>,
}

impl From<&Refusal> for KafkaRefusal {
    fn from(r: &Refusal) -> Self {
        Self {
            code: r.code,
            name: r.name.clone(),
            message: r.message.clone(),
            rule: r.rule.clone(),
            would_pass: r.would_pass.clone(),
        }
    }
}

// The three are modules for `#[serde(with = "..")]` (serde calls `serialize` in them), which ts-rs reads together with the
// field's `#[ts(as = "..")]` or `#[ts(type = "..")]`; a `serialize_with` it cannot read, and warns on every build.

/// Serialises a [`Refusal`] as a [`KafkaRefusal`] (`#[serde(with = "crate::api::settings::refusal")]`).
pub mod refusal {
    use super::{KafkaRefusal, Refusal, Serialize, Serializer};

    /// # Errors
    /// The serialiser's.
    pub fn serialize<S: Serializer>(r: &Refusal, s: S) -> Result<S::Ok, S::Error> {
        KafkaRefusal::from(r).serialize(s)
    }
}

/// Serialises an optional [`Refusal`] as a [`KafkaRefusal`] or `null`.
pub mod refusal_opt {
    use super::{KafkaRefusal, Refusal, Serialize, Serializer};

    /// # Errors
    /// The serialiser's.
    #[allow(clippy::ref_option)]
    pub fn serialize<S: Serializer>(r: &Option<Refusal>, s: S) -> Result<S::Ok, S::Error> {
        r.as_ref().map(KafkaRefusal::from).serialize(s)
    }
}

/// Serialises a config's source as the Settings table words it (`Static broker`).
pub mod source_word {
    use super::{ConfigSource, Serializer};

    /// # Errors
    /// The serialiser's.
    pub fn serialize<S: Serializer>(c: &ConfigSource, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(crate::gather::settings_configs::source_label(*c))
    }
}

/// `GET /api/v1/settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SettingsResponse {
    pub settings: SettingsView,
}

/// What the Edit dialog posts.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SettingsPost {
    pub key: String,
    /// The key the dialog was open on: a different `key` is the Select moving to another config.
    pub was: String,
    pub scope: String,
    pub value: String,
    /// Ask the broker whether it would take it (`validate_only`), and change nothing.
    pub check: bool,
}

/// What a check or a save came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SettingsOutcome {
    /// Saved: the rows as they are now, and who sees the value.
    Saved { saved: SettingsSaved },
    /// Kafka said no: its words, the rule, what would pass.
    Refused { refused: SettingsRefused },
    /// The broker would take it (`validate_only`): the dialog with the before and after.
    Checked { form: SettingsEditView },
    /// A field is wrong: the dialog with the sentence.
    Invalid { form: SettingsEditView },
    /// The Select moved to another config: its dialog, nothing asked of the broker.
    Switched { form: SettingsEditView },
}

impl From<SettingsResult> for SettingsOutcome {
    fn from(r: SettingsResult) -> Self {
        match r {
            SettingsResult::Saved(v) => Self::Saved { saved: *v },
            SettingsResult::Refused(v) => Self::Refused { refused: *v },
            SettingsResult::Checked(v) => Self::Checked { form: *v },
            SettingsResult::Invalid(v) => Self::Invalid { form: *v },
            SettingsResult::Switched(v) => Self::Switched { form: *v },
        }
    }
}

/// The longest text a field is read to.
const MOST: usize = 200;

fn cut(s: &str) -> String {
    s.chars().take(MOST).collect()
}

pub(super) async fn page(State(s): State<Shared>) -> Response {
    json(
        StatusCode::OK,
        &SettingsResponse {
            settings: s.source.settings().await,
        },
    )
}

pub(super) async fn edit_form(State(s): State<Shared>, RawQuery(q): RawQuery) -> Response {
    let f = crate::form::parse(q.as_deref().unwrap_or(""));
    let key = cut(crate::form::get(&f, "key").unwrap_or(""));
    let scope = crate::form::get(&f, "scope").unwrap_or("all");
    match s.source.settings_form(&key, scope).await {
        Some(v) => json(StatusCode::OK, &v),
        None => error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "The broker has no dynamic config by that name. Five can change while it runs; they are on Settings.",
        ),
    }
}

pub(super) async fn edit(State(s): State<Shared>, body: Bytes) -> Response {
    let Ok(p) = serde_json::from_slice::<SettingsPost>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "The change was not in the form the console sends.",
        );
    };
    let (key, was, scope, value) = (cut(&p.key), cut(&p.was), cut(&p.scope), cut(&p.value));
    let fields = SettingsFields {
        key: &key,
        was: if was.is_empty() { &key } else { &was },
        scope: &scope,
        value: &value,
    };
    let outcome = SettingsOutcome::from(s.source.settings_save(fields, p.check).await);
    json(StatusCode::OK, &outcome)
}
