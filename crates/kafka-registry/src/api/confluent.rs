//! Confluent's routes: what each answers, in Confluent's shapes.

use std::collections::BTreeMap;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::route::Route;
use super::{Call, Response, text};
use crate::error::{RegistryError, code};
use crate::registry::{RegisterResult, Registry, SchemaInput, SchemaView};

/// The body of a schema request.
// fuzz:parser registry_api
pub(crate) fn read_input(call: &Call<'_>) -> Result<SchemaInput, RegistryError> {
    let body = call.body()?;
    serde_json::from_slice::<Option<SchemaInput>>(body)
        .map(Option::unwrap_or_default)
        .map_err(|e| {
            RegistryError::http(400, format!("Unrecognized token in the request body: {e}"))
        })
}

/// The small body of a level or mode change: the first JSON value, as Go's decoder reads one.
fn read_setting<T: DeserializeOwned + Default>(call: &Call<'_>) -> Result<T, RegistryError> {
    let body = call.body()?;
    let first = serde_json::Deserializer::from_slice(body)
        .into_iter::<Option<T>>()
        .next();
    match first {
        Some(Ok(v)) => Ok(v.unwrap_or_default()),
        Some(Err(e)) => Err(RegistryError::new(
            422,
            code::INVALID_REQUEST,
            format!("Unrecognized token in the request body: {e}"),
        )),
        None => Err(RegistryError::new(
            422,
            code::INVALID_REQUEST,
            "Unrecognized token in the request body: EOF",
        )),
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct LevelBody {
    compatibility: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ModeBody {
    mode: Option<String>,
}

/// The subject prefix a listing asks for; `:*:` (every context) is every subject.
fn prefix(call: &Call<'_>) -> String {
    call.query("subjectPrefix")
        .filter(|p| p != ":*:")
        .unwrap_or_default()
}

/// A schema id in a path. One that is not a number is no route (404); one out of range is no schema.
fn schema_id(call: &Call<'_>) -> Result<u32, RegistryError> {
    let raw = call.at.id.as_deref().unwrap_or_default();
    let n = text::atoi(raw).ok_or_else(|| RegistryError::http(404, "HTTP 404 Not Found"))?;
    u32::try_from(n).map_err(|_| {
        RegistryError::new(404, code::SCHEMA_NOT_FOUND, format!("Schema {n} not found"))
    })
}

/// `offset` and `limit` on a list, as Confluent applies them to `GET /schemas`: a missing, negative or unreadable offset
/// is 0; a limit that is not positive is no limit.
fn page<T>(mut all: Vec<T>, call: &Call<'_>) -> Vec<T> {
    let num = |name| call.query(name).as_deref().and_then(text::atoi);
    if let Some(off) = num("offset").filter(|&o| o > 0) {
        let off = usize::try_from(off).unwrap_or(usize::MAX).min(all.len());
        all.drain(..off);
    }
    if let Some(lim) = num("limit").filter(|&l| l > 0) {
        all.truncate(usize::try_from(lim).unwrap_or(usize::MAX));
    }
    all
}

fn one<K: Serialize + Ord, V: Serialize>(key: K, value: V) -> BTreeMap<K, V> {
    BTreeMap::from([(key, value)])
}

/// Answers a Confluent route.
pub(crate) fn answer(
    r: &Registry,
    route: Route,
    call: &Call<'_>,
) -> Result<Response, RegistryError> {
    match route {
        Route::Root => Ok(Response::ok(&BTreeMap::<String, String>::new())),
        Route::Schemas
        | Route::Types
        | Route::SchemaById
        | Route::SchemaText
        | Route::IdSubjects
        | Route::IdVersions => schemas(r, route, call),
        Route::CompatVersion | Route::CompatLevel => compatibility(r, call),
        Route::GetConfig
        | Route::GetSubjectConfig
        | Route::PutConfig
        | Route::PutSubjectConfig
        | Route::DeleteConfig
        | Route::DeleteSubjectConfig => config(r, route, call),
        Route::GetMode
        | Route::GetSubjectMode
        | Route::PutMode
        | Route::PutSubjectMode
        | Route::DeleteSubjectMode => mode(r, route, call),
        _ => subjects(r, route, call),
    }
}

fn subjects(r: &Registry, route: Route, call: &Call<'_>) -> Result<Response, RegistryError> {
    let subject = call.subject();
    Ok(match route {
        Route::Subjects => Response::ok(&r.subjects(
            &prefix(call),
            call.flag("deleted"),
            call.flag("deletedOnly"),
        )),
        Route::Versions => Response::ok(&r.versions(subject, call.flag("deleted"))?),
        Route::Register => {
            let input = read_input(call)?;
            match r.register(subject, &input, call.flag("normalize"), &call.origin())? {
                RegisterResult::Id(id) => Response::ok(&one("id", id)),
                RegisterResult::Latest(latest) => Response::ok(&SchemaView {
                    subject: String::new(),
                    ..latest
                }),
            }
        }
        Route::Lookup => {
            let input = read_input(call)?;
            Response::ok(&r.lookup(
                subject,
                &input,
                call.flag("normalize"),
                call.flag("deleted"),
            )?)
        }
        Route::DeleteSubject => Response::ok(&r.delete_subject(subject, call.flag("permanent"))?),
        Route::Version => {
            Response::ok(&r.version(subject, call.version(), call.flag("deleted"))?)
        }
        Route::VersionSchema => Response::text(
            r.version(subject, call.version(), call.flag("deleted"))?
                .schema,
        ),
        Route::ReferencedBy => Response::ok(&r.referenced_by(subject, call.version())?),
        Route::DeleteVersion => {
            Response::ok(&r.delete_version(subject, call.version(), call.flag("permanent"))?)
        }
        _ => return Err(RegistryError::http(404, "HTTP 404 Not Found")),
    })
}

#[derive(Serialize)]
struct WithMaxId {
    #[serde(flatten)]
    view: SchemaView,
    #[serde(rename = "maxId")]
    max_id: u32,
}

#[derive(Serialize)]
struct SubjectVersion {
    subject: String,
    version: u32,
}

fn schemas(r: &Registry, route: Route, call: &Call<'_>) -> Result<Response, RegistryError> {
    Ok(match route {
        Route::Schemas => {
            let all = r.all_schemas(&prefix(call), call.flag("latestOnly"), call.flag("deleted"));
            Response::ok(&page(all, call))
        }
        Route::Types => Response::ok(&r.kinds()),
        Route::SchemaById => {
            let view = r.schema_by_id(schema_id(call)?)?;
            if call.flag("fetchMaxId") {
                Response::ok(&WithMaxId {
                    view,
                    max_id: r.max_id(),
                })
            } else {
                Response::ok(&view)
            }
        }
        Route::SchemaText => Response::text(r.schema_by_id(schema_id(call)?)?.schema),
        Route::IdSubjects => {
            let mut names: Vec<String> = Vec::new();
            for (subject, _) in r.id_versions(schema_id(call)?, call.flag("deleted"))? {
                if !names.contains(&subject) {
                    names.push(subject);
                }
            }
            Response::ok(&names)
        }
        Route::IdVersions => {
            let pairs = r.id_versions(schema_id(call)?, call.flag("deleted"))?;
            let out: Vec<SubjectVersion> = pairs
                .into_iter()
                .map(|(subject, version)| SubjectVersion { subject, version })
                .collect();
            Response::ok(&out)
        }
        _ => return Err(RegistryError::http(404, "HTTP 404 Not Found")),
    })
}

#[derive(Serialize)]
struct Verdict {
    is_compatible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    messages: Option<Vec<String>>,
}

fn compatibility(r: &Registry, call: &Call<'_>) -> Result<Response, RegistryError> {
    let input = read_input(call)?;
    let version = call.at.version.as_deref();
    let (_, found) = r.check(
        call.subject(),
        version,
        &input,
        call.flag("normalize"),
        None,
    )?;
    // An empty list when it fits, as Confluent sends it.
    let messages = call.flag("verbose").then(|| {
        found
            .iter()
            .flat_map(crate::registry::CompatFinding::messages)
            .collect()
    });
    Ok(Response::ok(&Verdict {
        is_compatible: found.is_empty(),
        messages,
    }))
}

fn config(r: &Registry, route: Route, call: &Call<'_>) -> Result<Response, RegistryError> {
    let subject = call.subject();
    Ok(match route {
        Route::GetConfig => Response::ok(&one("compatibilityLevel", r.level("", true)?.name())),
        Route::GetSubjectConfig => Response::ok(&one(
            "compatibilityLevel",
            r.level(subject, call.flag("defaultToGlobal"))?.name(),
        )),
        Route::PutConfig | Route::PutSubjectConfig => {
            let body: LevelBody = read_setting(call)?;
            match body.compatibility.filter(|c| !c.is_empty()) {
                // As Confluent: nothing to set, nothing changed.
                None => Response::ok(&BTreeMap::<String, String>::new()),
                Some(level) => {
                    r.set_level(subject, &level)?;
                    Response::ok(&one("compatibility", level))
                }
            }
        }
        Route::DeleteConfig | Route::DeleteSubjectConfig => {
            Response::ok(&one("compatibilityLevel", r.delete_level(subject)?.name()))
        }
        _ => return Err(RegistryError::http(404, "HTTP 404 Not Found")),
    })
}

fn mode(r: &Registry, route: Route, call: &Call<'_>) -> Result<Response, RegistryError> {
    let subject = call.subject();
    Ok(match route {
        Route::GetMode => Response::ok(&one("mode", r.mode("", true)?.name())),
        Route::GetSubjectMode => Response::ok(&one(
            "mode",
            r.mode(subject, call.flag("defaultToGlobal"))?.name(),
        )),
        Route::PutMode | Route::PutSubjectMode => {
            let body: ModeBody = read_setting(call)?;
            let set = r.set_mode(subject, body.mode.as_deref().unwrap_or_default())?;
            Response::ok(&one("mode", set.name()))
        }
        Route::DeleteSubjectMode => Response::ok(&one("mode", r.delete_mode(subject)?.name())),
        _ => return Err(RegistryError::http(404, "HTTP 404 Not Found")),
    })
}
