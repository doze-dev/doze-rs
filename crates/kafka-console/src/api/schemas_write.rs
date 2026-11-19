//! The Schemas pages' writes and checks: the live compatibility check of a draft (nothing stored), registering it, a
//! subject's level and mode, deleting a version or a subject (soft, then for good), and Try (a JSON value encoded with a
//! version, nothing sent). Each is the registry's own call in this process, so what it refuses is refused in its words
//! and code, as Confluent's REST API would. Every answer is a 200 with an `outcome`; a body that is not what a write takes is
//! `BAD_REQUEST`.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use kafka_registry::format::{CompatReason, SchemaType};
use kafka_registry::registry::{CompatFinding, Origin, RegisterResult};
use kafka_registry::store::Reference;
use kafka_registry::{Registry, RegistryError, SchemaInput};
use serde::{Deserialize, Serialize};

use super::{off, subject_of};
use crate::api::{error, json};
use crate::state::Shared;

/// A reference as the register form sends it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RefPost {
    pub name: String,
    pub subject: String,
    pub version: i64,
}

/// `POST /api/v1/schemas/check` and `/register`: a draft.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SchemaPost {
    pub subject: String,
    /// `AVRO`, `JSON`, `PROTOBUF`.
    pub schema_type: String,
    pub schema: String,
    #[serde(default)]
    pub references: Vec<RefPost>,
    #[serde(default)]
    pub normalize: bool,
    /// A level to check at instead of the subject's (the diff's "at another level").
    #[serde(default)]
    pub level: Option<String>,
}

/// A change to the draft that would remove a reason, made here and sent back whole: nothing is registered by it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FixView {
    /// `Add "default": null to note`.
    pub label: String,
    /// The draft with the change made.
    pub schema: String,
}

/// One reason, in the registry's (Confluent's) words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReasonView {
    /// Confluent's name for it: `READER_FIELD_MISSING_DEFAULT_VALUE`.
    pub kind: String,
    pub path: String,
    pub description: String,
    pub fix: Option<FixView>,
}

/// The reasons against one existing version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FindingView {
    pub version: u32,
    pub level: String,
    pub reasons: Vec<ReasonView>,
}

/// What a check came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CheckAnswer {
    /// A new subject: nothing to compare with; it would be version 1.
    First { kind: String },
    Compatible {
        level: String,
        compared: Vec<u32>,
        kind: String,
    },
    Incompatible {
        level: String,
        compared: Vec<u32>,
        findings: Vec<FindingView>,
    },
    /// The registry refused the draft itself (42201 invalid schema, 42208 invalid subject, ...).
    Invalid { error_code: u32, message: String },
}

/// What a write came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SchemaWrite {
    /// Registered: the id, the version, whether the subject already held it, and the other subjects holding the id.
    Registered {
        id: u32,
        version: u32,
        existed: bool,
        shared: Vec<String>,
    },
    /// A level or mode set or removed: what is in force now.
    Configured { level: String, mode: String },
    /// Versions deleted (soft, or for good).
    Deleted { versions: Vec<u32>, permanent: bool },
    /// The registry refused it: Confluent's status, code and sentence, and for a 409 the reasons.
    Refused {
        status: u16,
        error_code: u32,
        message: String,
        findings: Vec<FindingView>,
    },
}

pub(super) fn bad_body() -> Response {
    error(
        StatusCode::BAD_REQUEST,
        "BAD_REQUEST",
        "The request's body is not what this write takes.",
    )
}

fn refused(e: &RegistryError, findings: Vec<FindingView>) -> Response {
    json(
        StatusCode::OK,
        &SchemaWrite::Refused {
            status: e.status,
            error_code: e.error_code,
            message: e.message.clone(),
            findings,
        },
    )
}

fn input(p: &SchemaPost) -> SchemaInput {
    SchemaInput {
        schema: Some(p.schema.clone()),
        schema_type: Some(p.schema_type.clone()),
        references: Some(
            p.references
                .iter()
                .map(|r| Reference {
                    name: r.name.clone(),
                    subject: r.subject.clone(),
                    version: r.version,
                })
                .collect(),
        ),
        ..SchemaInput::default()
    }
}

fn reason(p: &SchemaPost, r: &CompatReason) -> ReasonView {
    ReasonView {
        kind: r.kind.clone(),
        path: r.path.clone(),
        description: r.description.clone(),
        fix: super::fix::fix(p, r),
    }
}

fn findings(p: &SchemaPost, found: &[CompatFinding]) -> Vec<FindingView> {
    found
        .iter()
        .map(|f| FindingView {
            version: f.version,
            level: f.level.name().to_owned(),
            reasons: f.details.iter().map(|r| reason(p, r)).collect(),
        })
        .collect()
}

fn registry(s: &Shared) -> Option<std::sync::Arc<Registry>> {
    s.source.schemas().registry().cloned()
}

/// `POST /api/v1/schemas/check`: the draft against the subject's versions at its level (or the one asked), stored nowhere.
pub(super) async fn check(State(s): State<Shared>, body: Bytes) -> Response {
    let Some(r) = registry(&s) else { return off() };
    let Ok(p) = serde_json::from_slice::<SchemaPost>(&body) else {
        return bad_body();
    };
    let kind = SchemaType::named(&p.schema_type)
        .map_or_else(|| p.schema_type.clone(), |k| k.name().to_owned());
    let answer = match r.check(
        &p.subject,
        None,
        &input(&p),
        p.normalize,
        p.level.as_deref(),
    ) {
        Err(e) => CheckAnswer::Invalid {
            error_code: e.error_code,
            message: e.message,
        },
        Ok((compared, _)) if compared.is_empty() && r.versions(&p.subject, false).is_err() => {
            CheckAnswer::First { kind }
        }
        Ok((compared, found)) if found.is_empty() => CheckAnswer::Compatible {
            level: r.level_for(&p.subject, p.level.as_deref()),
            compared,
            kind,
        },
        Ok((compared, found)) => CheckAnswer::Incompatible {
            level: r.level_for(&p.subject, p.level.as_deref()),
            compared,
            findings: findings(&p, &found),
        },
    };
    json(StatusCode::OK, &answer)
}

fn console_origin() -> Origin {
    Origin {
        user: String::new(),
        from: "console".to_owned(),
        agent: crate::schemas::CONSOLE_AGENT.to_owned(),
    }
}

/// `POST /api/v1/schemas/register`: registers the draft, as `POST /subjects/{s}/versions` would.
pub(super) async fn register(State(s): State<Shared>, body: Bytes) -> Response {
    let Some(r) = registry(&s) else { return off() };
    let Ok(p) = serde_json::from_slice::<SchemaPost>(&body) else {
        return bad_body();
    };
    let input = input(&p);
    let before = r.lookup(&p.subject, &input, p.normalize, false).ok();
    match r.register(&p.subject, &input, p.normalize, &console_origin()) {
        Ok(done @ (RegisterResult::Id(_) | RegisterResult::Full(_))) => {
            let id = done.id();
            let version = match &done {
                RegisterResult::Full(v) => v.version,
                _ => r
                    .lookup(&p.subject, &input, p.normalize, false)
                    .map_or(0, |v| v.version),
            };
            let shared = r
                .id_versions(id, false)
                .unwrap_or_default()
                .into_iter()
                .map(|(subject, _)| subject)
                .filter(|x| *x != p.subject)
                .collect();
            json(
                StatusCode::OK,
                &SchemaWrite::Registered {
                    id,
                    version,
                    existed: before.is_some(),
                    shared,
                },
            )
        }
        Ok(RegisterResult::Latest(_)) => refused(
            &RegistryError::invalid_schema("Invalid schema: schema may not be empty"),
            Vec::new(),
        ),
        Err(e) if e.error_code == 409 => {
            let found = r
                .check(&p.subject, None, &input, p.normalize, None)
                .map(|(_, f)| f)
                .unwrap_or_default();
            refused(&e, findings(&p, &found))
        }
        Err(e) => refused(&e, Vec::new()),
    }
}

/// A level or mode to set: a name, or `default` to remove the subject's own.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigPost {
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

fn configure(r: &Registry, subject: &str, p: &ConfigPost) -> Result<(), RegistryError> {
    match p.level.as_deref() {
        Some("default") if !subject.is_empty() => match r.delete_level(subject) {
            Err(e) if e.error_code == 40408 => {}
            other => {
                other?;
            }
        },
        Some(level) => r.set_level(subject, level)?,
        None => {}
    }
    match p.mode.as_deref() {
        Some("default") if !subject.is_empty() => match r.delete_mode(subject) {
            Err(e) if e.error_code == 40409 => {}
            other => {
                other?;
            }
        },
        // A mode is set only when it changes, as the Go console did: setting READWRITE again would give the subject one.
        Some(mode) if r.mode(subject, true).map(|m| m.name()) != Ok(mode) => {
            r.set_mode(subject, mode, false)?;
        }
        _ => {}
    }
    Ok(())
}

fn configured(r: &Registry, subject: &str, p: &ConfigPost) -> Response {
    match configure(r, subject, p) {
        Ok(()) => json(
            StatusCode::OK,
            &SchemaWrite::Configured {
                level: r
                    .level(subject, true)
                    .map(|l| l.name().to_owned())
                    .unwrap_or_default(),
                mode: r
                    .mode(subject, true)
                    .map(|m| m.name().to_owned())
                    .unwrap_or_default(),
            },
        ),
        Err(e) => refused(&e, Vec::new()),
    }
}

/// `POST /api/v1/schemas/subjects/{subject}/config`.
pub(super) async fn config(
    State(s): State<Shared>,
    Path(raw): Path<String>,
    body: Bytes,
) -> Response {
    let Some(r) = registry(&s) else { return off() };
    let Ok(p) = serde_json::from_slice::<ConfigPost>(&body) else {
        return bad_body();
    };
    configured(&r, &subject_of(&raw), &p)
}

/// `POST /api/v1/schemas/config`: the global level and mode.
pub(super) async fn global_config(State(s): State<Shared>, body: Bytes) -> Response {
    let Some(r) = registry(&s) else { return off() };
    let Ok(p) = serde_json::from_slice::<ConfigPost>(&body) else {
        return bad_body();
    };
    configured(&r, "", &p)
}

/// `POST /api/v1/schemas/subjects/{subject}/delete`: one version or every one; soft, or `permanent` once soft-deleted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DeletePost {
    /// A version number; none for the whole subject.
    #[serde(default)]
    pub version: Option<u32>,
    #[serde(default)]
    pub permanent: bool,
    /// For a subject deleted for good: its name, typed.
    #[serde(default)]
    pub confirm: Option<String>,
}

pub(super) async fn delete(
    State(s): State<Shared>,
    Path(raw): Path<String>,
    body: Bytes,
) -> Response {
    let Some(r) = registry(&s) else { return off() };
    let Ok(p) = serde_json::from_slice::<DeletePost>(&body) else {
        return bad_body();
    };
    let subject = subject_of(&raw);
    let done = match p.version {
        Some(v) => r
            .delete_version(&subject, &v.to_string(), p.permanent)
            .map(|n| vec![n]),
        None => {
            if p.permanent && p.confirm.as_deref() != Some(subject.as_str()) {
                return error(
                    StatusCode::BAD_REQUEST,
                    "CONFIRM",
                    "Type the subject's name to delete it for good.",
                );
            }
            r.delete_subject(&subject, p.permanent)
        }
    };
    match done {
        Ok(versions) => json(
            StatusCode::OK,
            &SchemaWrite::Deleted {
                versions,
                permanent: p.permanent,
            },
        ),
        Err(e) => refused(&e, Vec::new()),
    }
}
