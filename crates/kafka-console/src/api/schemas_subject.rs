//! One subject: its versions, the version on screen laid out with its references both ways, its levels.

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use kafka_registry::Registry;
use kafka_registry::codec::pretty;
use kafka_registry::format::SchemaType;
use serde::Serialize;

use super::layout::laid_out;
use super::{SubjectRow, no_subject, off, row, topic_names};
use crate::api::json;
use crate::form::{get as field, parse};
use crate::state::Shared;

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
        (
            "BACKWARD",
            "Readers on the new schema can read data written with the previous version.",
        ),
        (
            "BACKWARD_TRANSITIVE",
            "The same, against every earlier version, not only the last.",
        ),
        (
            "FORWARD",
            "Readers on the previous version can read data written with the new schema.",
        ),
        ("FORWARD_TRANSITIVE", "The same, for every earlier version."),
        (
            "FULL",
            "Both BACKWARD and FORWARD, against the previous version.",
        ),
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

fn shown(r: &Registry, subject: &str, version: &str) -> Option<VersionShown> {
    let info = r.version_info(subject, version).ok()?;
    let v = &info.view;
    let kind = v.schema_type.unwrap_or(SchemaType::Avro);
    let holders = |pairs: Vec<(String, u32)>, id: u32| -> Vec<Holder> {
        pairs
            .into_iter()
            .map(|(subject, version)| Holder {
                id,
                subject,
                version,
            })
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

pub(super) async fn subject(
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
    let Some(shown) =
        shown(&r, &name, &version).or_else(|| shown(&r, &name, &state.latest_version.to_string()))
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
