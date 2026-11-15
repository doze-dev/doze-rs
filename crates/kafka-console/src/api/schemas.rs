//! The JSON API of the Schemas pages (slice 10d): the subjects, one subject with its versions, a version's schema laid out,
//! its references both ways, its levels, where it is used, and the diff of two versions. Every answer comes from the
//! registry in this process ([`crate::schemas::Schemas`]); none goes over HTTP to the address the registry serves. With no
//! registry each answers `SCHEMAS_OFF` (the page says why from `GET /api/v1/schemas`, which always answers).
//!
//! A subject is hostile input: it is any text a client registered, and it comes back as JSON text, never as markup.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use kafka_registry::Registry;
use kafka_registry::format::SchemaType;
use kafka_registry::registry::SubjectState;
use serde::Serialize;

use super::topics::missing;
use super::{error, json};
use crate::schemas::{Schemas, named_topics};
use crate::state::Shared;

#[path = "schemas_diff.rs"]
pub mod diff;
#[path = "schemas_fix.rs"]
mod fix;
#[path = "schemas_layout.rs"]
pub mod layout;
#[path = "schemas_subject.rs"]
pub mod subject;
#[path = "schemas_try.rs"]
pub mod try_it;
#[path = "schemas_write.rs"]
pub mod write;

pub use layout::laid_out;
pub(crate) use subject::subject_of;
pub use subject::{Holder, LevelWord, RefRow, SubjectResponse, VersionRow, VersionShown, levels};

/// Why there is no registry, as the page says it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SchemasOff {
    /// `Not running`, `Did not start`, `Not in this build`.
    pub title: String,
    pub reason: String,
    /// The command that starts the broker with the registry on.
    pub command: String,
}

/// One subject in the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SubjectRow {
    pub subject: String,
    /// `AVRO`, `JSON`, `PROTOBUF` (of the latest version).
    pub kind: String,
    /// Versions that are not deleted.
    pub versions: u32,
    pub deleted_versions: u32,
    /// Every version is soft-deleted: the subject is not listed by `GET /subjects`.
    pub deleted: bool,
    pub latest_version: u32,
    pub latest_id: u32,
    pub level: String,
    /// The subject has a level of its own (else the global one applies).
    pub level_is_own: bool,
    pub mode: String,
    pub mode_is_own: bool,
    /// The references of the latest version.
    pub references: usize,
    /// The topics its name says it is used on, and as what (`value`, `key`, `record`).
    pub topics: Vec<(String, String)>,
    /// RFC 3339, UTC.
    pub last_registered: Option<String>,
}

/// `GET /api/v1/schemas`: the list, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SchemasResponse {
    pub off: Option<SchemasOff>,
    /// `http://127.0.0.1:8081`.
    pub url: Option<String>,
    /// Clients sign in with the broker's SASL users.
    pub basic_auth: bool,
    pub level: String,
    pub mode: String,
    pub next_id: u32,
    /// How many schema ids resolve.
    pub ids: u32,
    pub subjects: Vec<SubjectRow>,
    /// The languages this build reads.
    pub kinds: Vec<String>,
    /// The broker's address, for the snippets.
    pub listen: String,
    /// A topic to name in the empty state's example.
    pub example_topic: Option<String>,
}

pub(crate) fn kind_word(k: Option<SchemaType>) -> String {
    k.unwrap_or(SchemaType::Avro).name().to_owned()
}

pub(crate) fn row(r: &Registry, s: &SubjectState, topics: &[String]) -> SubjectRow {
    let references = r
        .version(&s.subject, &s.latest_version.to_string(), true)
        .map_or(0, |v| v.references.len());
    SubjectRow {
        subject: s.subject.clone(),
        kind: kind_word(s.kind),
        versions: s.versions,
        deleted_versions: s.deleted_versions,
        deleted: s.deleted,
        latest_version: s.latest_version,
        latest_id: s.latest_id,
        level: s.level.clone(),
        level_is_own: s.level_is_own,
        mode: s.mode.clone(),
        mode_is_own: s.mode_is_own,
        references,
        topics: named_topics(&s.subject, topics)
            .into_iter()
            .map(|(t, p)| (t, p.to_owned()))
            .collect(),
        last_registered: s.last_registered.clone(),
    }
}

/// The refusal of a schema call with no registry.
pub(crate) fn off() -> Response {
    error(
        StatusCode::CONFLICT,
        "SCHEMAS_OFF",
        "This console has no schema registry: the Schemas page says why.",
    )
}

/// The topic names (internal ones left out): what subjects are matched against.
pub(crate) async fn topic_names(s: &Shared) -> Vec<String> {
    s.source
        .topics("", false, None)
        .await
        .rows
        .into_iter()
        .map(|r| r.name)
        .collect()
}

async fn command(s: &Shared) -> String {
    let data = s.source.home(None).await.instance.data_dir;
    if data.is_empty() {
        return "doze-kafka --schema-registry".to_owned();
    }
    let quoted = if data.contains([' ', '\'', '"']) {
        format!("'{}'", data.replace('\'', "'\\''"))
    } else {
        data
    };
    format!("doze-kafka --data-dir {quoted} --schema-registry")
}

async fn list(State(s): State<Shared>) -> Response {
    let listen = s.source.listen();
    let topics = topic_names(&s).await;
    let example_topic = topics.iter().find(|t| !t.starts_with("__")).cloned();
    let (registry, url, basic_auth) = match s.source.schemas() {
        Schemas::On {
            registry,
            url,
            basic_auth,
        } => (registry.clone(), url.clone(), *basic_auth),
        Schemas::Off(why) => {
            return json(
                StatusCode::OK,
                &SchemasResponse {
                    off: Some(SchemasOff {
                        title: why.title().to_owned(),
                        reason: why.reason(),
                        command: command(&s).await,
                    }),
                    url: None,
                    basic_auth: false,
                    level: String::new(),
                    mode: String::new(),
                    next_id: 0,
                    ids: 0,
                    subjects: Vec::new(),
                    kinds: Vec::new(),
                    listen,
                    example_topic,
                },
            );
        }
    };
    let o = registry.overview();
    let ids = o.ids_in_use.iter().map(|[a, b]| b - a + 1).sum();
    json(
        StatusCode::OK,
        &SchemasResponse {
            off: None,
            url: Some(url),
            basic_auth,
            level: o.level.clone(),
            mode: o.mode.clone(),
            next_id: o.next_id,
            ids,
            subjects: o
                .subjects
                .iter()
                .map(|x| row(&registry, x, &topics))
                .collect(),
            kinds: registry
                .kinds()
                .iter()
                .map(|k| k.name().to_owned())
                .collect(),
            listen,
            example_topic,
        },
    )
}

pub(crate) fn no_subject(name: &str) -> Response {
    let shown: String = name.chars().take(120).collect();
    missing(
        "SUBJECT_NOT_FOUND",
        &format!(
            "Subject '{shown}' not found. The registry knows no subject by that name (40401)."
        ),
    )
}

/// The routes of the Schemas pages under `web`.
pub fn routes(web: &crate::base::Base) -> axum::Router<Shared> {
    axum::Router::new()
        .route(&web.url("/api/v1/schemas"), get(list))
        .route(
            &web.url("/api/v1/schemas/subjects/{subject}"),
            get(subject::subject),
        )
        .route(
            &web.url("/api/v1/schemas/subjects/{subject}/diff"),
            get(diff::diff),
        )
        .route(
            &web.url("/api/v1/schemas/subjects/{subject}/config"),
            post(write::config),
        )
        .route(
            &web.url("/api/v1/schemas/subjects/{subject}/delete"),
            post(write::delete),
        )
        .route(&web.url("/api/v1/schemas/check"), post(write::check))
        .route(&web.url("/api/v1/schemas/register"), post(write::register))
        .route(&web.url("/api/v1/schemas/try"), post(try_it::try_value))
        .route(
            &web.url("/api/v1/schemas/config"),
            post(write::global_config),
        )
}
