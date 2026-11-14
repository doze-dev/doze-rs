//! The JSON API of the Schemas pages (slice 10d): the subjects, one subject with its versions, a version's schema laid out,
//! its references both ways, its levels, where it is used, and the diff of two versions. Every answer comes from the
//! registry in this process ([`crate::schemas::Schemas`]); none goes over HTTP to the address the registry serves. With no
//! registry each answers `SCHEMAS_OFF` (the page says why from `GET /api/v1/schemas`, which always answers).
//!
//! A subject is hostile input: it is any text a client registered, and it comes back as JSON text, never as markup.

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use kafka_registry::Registry;
use kafka_registry::codec::pretty;
use kafka_registry::format::SchemaType;
use kafka_registry::json::Json;
use kafka_registry::registry::SubjectState;
use serde::Serialize;

use super::topics::missing;
use super::{error, json};
use crate::form::{get as field, parse};
use crate::schemas::{Schemas, named_topics};
use crate::state::Shared;

#[path = "schemas_diff.rs"]
pub mod diff;
#[path = "schemas_try.rs"]
pub mod try_it;
#[path = "schemas_write.rs"]
pub mod write;

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
            subjects: o.subjects.iter().map(|x| row(&registry, x, &topics)).collect(),
            kinds: registry.kinds().iter().map(|k| k.name().to_owned()).collect(),
            listen,
            example_topic,
        },
    )
}

/// A reference, as a version names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RefRow {
    /// The name the schema uses for it (an Avro full name, a `$ref`, an import path).
    pub name: String,
    pub subject: String,
    pub version: i64,
}

/// A version that holds an id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Holder {
    pub id: u32,
    pub subject: String,
    pub version: u32,
}

/// One version in the subject's table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VersionRow {
    pub version: u32,
    pub id: u32,
    pub deleted: bool,
    pub registered_at: Option<String>,
    /// Who: the user it signed in as, else the first word of its User-Agent, else its address.
    pub registered_by: String,
    pub references: usize,
}

/// The version on screen, completely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VersionShown {
    pub version: u32,
    pub id: u32,
    pub deleted: bool,
    pub kind: String,
    /// Laid out for reading: JSON two spaces a level, Protobuf as Confluent prints it.
    pub schema: String,
    /// As stored (what `GET /subjects/{s}/versions/{v}/schema` answers).
    pub stored: String,
    pub references: Vec<RefRow>,
    /// The schemas that reference this version.
    pub referenced_by: Vec<Holder>,
    /// Other subjects (and versions) that hold the same id.
    pub same_id: Vec<Holder>,
    /// Avro with its referenced types written out, when it has references.
    pub resolved: Option<String>,
    pub registered_at: Option<String>,
    pub registered_by: String,
    /// What a JSON value for it looks like: the start of the Try box.
    pub sample: String,
}

/// A level, with what it means.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LevelWord {
    pub name: String,
    pub sentence: String,
}

/// The seven levels, in the Go edition's words.
#[must_use]
pub fn levels() -> Vec<LevelWord> {
    [
        ("BACKWARD", "Readers on the new schema can read data written with the previous version."),
        ("BACKWARD_TRANSITIVE", "The same, against every earlier version, not only the last."),
        ("FORWARD", "Readers on the previous version can read data written with the new schema."),
        ("FORWARD_TRANSITIVE", "The same, for every earlier version."),
        ("FULL", "Both BACKWARD and FORWARD, against the previous version."),
        ("FULL_TRANSITIVE", "Both, against every earlier version."),
        ("NONE", "Nothing is checked. Any schema may follow."),
    ]
    .into_iter()
    .map(|(n, s)| LevelWord {
        name: n.to_owned(),
        sentence: s.to_owned(),
    })
    .collect()
}

/// `GET /api/v1/schemas/subjects/{subject}?version=`: one subject (or `{missing}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SubjectResponse {
    pub row: SubjectRow,
    /// Newest first, soft-deleted ones included.
    pub versions: Vec<VersionRow>,
    pub shown: VersionShown,
    pub levels: Vec<LevelWord>,
    pub global_level: String,
    pub global_mode: String,
    pub url: String,
}

/// A subject as the address carries it (`!.` for `.`, as for a topic).
pub(crate) fn subject_of(raw: &str) -> String {
    match raw {
        "!." | "!.." => raw.trim_start_matches('!').to_owned(),
        other => other.to_owned(),
    }
}

/// Who registered a version, in a word.
fn by(user: &str, agent: &str, from: &str) -> String {
    if !user.is_empty() {
        return user.to_owned();
    }
    if agent == crate::schemas::CONSOLE_AGENT {
        return "this console".to_owned();
    }
    agent
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| from.to_owned())
}

/// A schema's text laid out for reading.
#[must_use]
pub fn laid_out(kind: SchemaType, text: &str) -> String {
    match kind {
        SchemaType::Protobuf => text.to_owned(),
        _ => Json::parse(text).map_or_else(|_| text.to_owned(), |j| pretty(&j)),
    }
}

fn shown(r: &Registry, subject: &str, version: &str) -> Option<VersionShown> {
    let info = r.version_info(subject, version).ok()?;
    let v = &info.view;
    let kind = v.schema_type.unwrap_or(SchemaType::Avro);
    let holders = |pairs: Vec<(String, u32)>, id: u32| -> Vec<Holder> {
        pairs
            .into_iter()
            .map(|(subject, version)| Holder { id, subject, version })
            .collect()
    };
    let referenced_by = r
        .referenced_by(subject, &v.version.to_string())
        .unwrap_or_default()
        .into_iter()
        .flat_map(|id| holders(r.id_versions(id, false).unwrap_or_default(), id))
        .collect();
    let same_id = holders(r.id_versions(v.id, false).unwrap_or_default(), v.id)
        .into_iter()
        .filter(|h| h.subject != subject)
        .collect();
    let resolved = (!v.references.is_empty())
        .then(|| r.resolved(subject, &v.version.to_string()).ok())
        .flatten()
        .filter(|x| x.inlined)
        .map(|x| laid_out(kind, &x.schema));
    let sample = r
        .codec(v.id)
        .ok()
        .map(|c| c.sample())
        .map_or_else(|| "{}".to_owned(), |j| pretty(&j));
    Some(VersionShown {
        version: v.version,
        id: v.id,
        deleted: info.deleted,
        kind: kind.name().to_owned(),
        schema: laid_out(kind, &v.schema),
        stored: v.schema.clone(),
        references: v
            .references
            .iter()
            .map(|x| RefRow {
                name: x.name.clone(),
                subject: x.subject.clone(),
                version: x.version,
            })
            .collect(),
        referenced_by,
        same_id,
        resolved,
        registered_by: by(&info.registered_by, &info.agent, &info.from),
        registered_at: info.registered_at,
        sample,
    })
}

async fn subject(
    State(s): State<Shared>,
    Path(raw): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    let Some(r) = s.source.schemas().registry().cloned() else {
        return off();
    };
    let name = subject_of(&raw);
    let f = parse(q.as_deref().unwrap_or(""));
    let o = r.overview();
    let Some(state) = o.subjects.iter().find(|x| x.subject == name) else {
        return no_subject(&name);
    };
    let version = field(&f, "version")
        .filter(|v| v.parse::<u32>().is_ok())
        .map_or_else(|| state.latest_version.to_string(), str::to_owned);
    let Some(shown) = shown(&r, &name, &version).or_else(|| shown(&r, &name, &state.latest_version.to_string()))
    else {
        return no_subject(&name);
    };
    let topics = topic_names(&s).await;
    let versions = versions(&r, &name);
    json(
        StatusCode::OK,
        &SubjectResponse {
            row: row(&r, state, &topics),
            versions,
            shown,
            levels: levels(),
            global_level: o.level.clone(),
            global_mode: o.mode.clone(),
            url: s.source.schemas().url().unwrap_or_default().to_owned(),
        },
    )
}

pub(crate) fn versions(r: &Registry, subject: &str) -> Vec<VersionRow> {
    let mut out: Vec<VersionRow> = r
        .versions(subject, true)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|n| r.version_info(subject, &n.to_string()).ok())
        .map(|i| VersionRow {
            version: i.view.version,
            id: i.view.id,
            deleted: i.deleted,
            references: i.view.references.len(),
            registered_by: by(&i.registered_by, &i.agent, &i.from),
            registered_at: i.registered_at,
        })
        .collect();
    out.reverse();
    out
}

pub(crate) fn no_subject(name: &str) -> Response {
    let shown: String = name.chars().take(120).collect();
    missing(
        "SUBJECT_NOT_FOUND",
        &format!("Subject '{shown}' not found. The registry knows no subject by that name (40401)."),
    )
}

/// The routes of the Schemas pages under `web`.
pub fn routes(web: &crate::base::Base) -> axum::Router<Shared> {
    axum::Router::new()
        .route(&web.url("/api/v1/schemas"), get(list))
        .route(&web.url("/api/v1/schemas/subjects/{subject}"), get(subject))
        .route(&web.url("/api/v1/schemas/subjects/{subject}/diff"), get(diff::diff))
        .route(&web.url("/api/v1/schemas/subjects/{subject}/config"), post(write::config))
        .route(&web.url("/api/v1/schemas/subjects/{subject}/delete"), post(write::delete))
        .route(&web.url("/api/v1/schemas/check"), post(write::check))
        .route(&web.url("/api/v1/schemas/register"), post(write::register))
        .route(&web.url("/api/v1/schemas/try"), post(try_it::try_value))
        .route(&web.url("/api/v1/schemas/config"), post(write::global_config))
}
