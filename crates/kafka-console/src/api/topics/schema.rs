//! A topic's schemas: the subjects named for it (TopicNameStrategy `<topic>-key` and `<topic>-value`, TopicRecordNameStrategy
//! `<topic>-<record>`), and the schemas its newest records carry by id (which finds a RecordNameStrategy subject, named for
//! its record alone). `GET /api/v1/topics/{topic}/schemas`.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use kafka_registry::format::SchemaType;
use serde::Serialize;

use super::{no_topic, valid};
use crate::api::json;
use crate::api::schemas::laid_out;
use crate::model::MessageQuery;
use crate::schemas::named_topics;
use crate::state::Shared;

/// A subject a topic uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TopicSubject {
    pub subject: String,
    /// `value`, `key`, `record` (named for a record on this topic), or `carried` (only its ids are on the records).
    pub position: String,
    pub kind: String,
    pub latest_version: u32,
    pub latest_id: u32,
    pub level: String,
    /// The latest version laid out.
    pub schema: String,
    /// Of the newest records read, how many carry one of its ids.
    pub carried: usize,
}

/// `GET /api/v1/topics/{topic}/schemas`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TopicSchemas {
    pub topic: String,
    /// Why there is no registry, when there is none.
    pub off: Option<String>,
    pub subjects: Vec<TopicSubject>,
    /// How many of the newest records were read for ids.
    pub scanned: usize,
    /// Ids the records carry that no subject holds here.
    pub unknown: Vec<u32>,
}

fn subject_row(r: &kafka_registry::Registry, subject: &str, position: &str, carried: usize) -> Option<TopicSubject> {
    let v = r.version(subject, "latest", false).ok()?;
    let kind = v.schema_type.unwrap_or(SchemaType::Avro);
    Some(TopicSubject {
        subject: subject.to_owned(),
        position: position.to_owned(),
        kind: kind.name().to_owned(),
        latest_version: v.version,
        latest_id: v.id,
        level: r.level_for(subject, None),
        schema: laid_out(kind, &v.schema),
        carried,
    })
}

/// The ids the records carry, keys and values, and how often.
fn carried_ids(rows: &[crate::model::RecordRow]) -> BTreeMap<u32, usize> {
    let mut ids: BTreeMap<u32, usize> = BTreeMap::new();
    for row in rows {
        for shown in [&row.key, &row.value] {
            if let Some(f) = &shown.framed
                && let Ok((id, _)) = kafka_registry::wire::split(&f.bytes)
            {
                *ids.entry(id).or_default() += 1;
            }
        }
    }
    ids
}

pub(crate) async fn schemas(State(s): State<Shared>, Path(name): Path<String>) -> Response {
    if !valid(&name) {
        return no_topic(&name);
    }
    let Some(view) = s.source.messages(&name, MessageQuery::default(), vec![]).await else {
        return no_topic(&name);
    };
    let Some(r) = s.source.schemas().registry().cloned() else {
        let why = match s.source.schemas() {
            crate::schemas::Schemas::Off(o) => o.reason(),
            crate::schemas::Schemas::On { .. } => String::new(),
        };
        return json(
            StatusCode::OK,
            &TopicSchemas {
                topic: name,
                off: Some(why),
                subjects: Vec::new(),
                scanned: 0,
                unknown: Vec::new(),
            },
        );
    };
    let ids = carried_ids(&view.rows);
    let mut carried: BTreeMap<String, usize> = BTreeMap::new();
    let mut unknown = Vec::new();
    for (id, n) in &ids {
        match r.id_versions(*id, false) {
            Ok(held) if !held.is_empty() => {
                let mut subjects: Vec<String> = held.into_iter().map(|(s, _)| s).collect();
                subjects.dedup();
                for subject in subjects {
                    *carried.entry(subject).or_default() += n;
                }
            }
            _ => unknown.push(*id),
        }
    }
    let topic = [name.clone()];
    let mut subjects: Vec<TopicSubject> = r
        .subjects("", false, false)
        .into_iter()
        .filter_map(|subject| {
            let named = named_topics(&subject, &topic).into_iter().next().map(|(_, p)| p);
            let n = carried.get(&subject).copied().unwrap_or(0);
            let position = named.or((n > 0).then_some("carried"))?;
            subject_row(&r, &subject, position, n)
        })
        .collect();
    let order = |p: &str| match p {
        "value" => 0,
        "key" => 1,
        "record" => 2,
        _ => 3,
    };
    subjects.sort_by(|a, b| order(&a.position).cmp(&order(&b.position)).then(a.subject.cmp(&b.subject)));
    json(
        StatusCode::OK,
        &TopicSchemas {
            topic: name,
            off: None,
            subjects,
            scanned: view.rows.len(),
            unknown,
        },
    )
}

/// The subject of each topic's values (else keys), for the topics list.
#[must_use]
pub fn list_subjects(s: &crate::schemas::Schemas, topics: &[String]) -> Vec<(String, String)> {
    let Some(r) = s.registry() else {
        return Vec::new();
    };
    let subjects = r.subjects("", false, false);
    topics
        .iter()
        .filter_map(|t| crate::schemas::subject_of(t, &subjects).map(|x| (t.clone(), x.to_owned())))
        .collect()
}
