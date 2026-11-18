//! Confluent's routes: what each answers, in Confluent's shapes.

use std::collections::BTreeMap;

use serde::Serialize;

use super::body::{self, Bound};
use super::route::Route;
use super::{Call, Response, text};
use crate::error::{RegistryError, code};
use crate::registry::{ConfigView, RegisterResult, Registry, SchemaInput, SchemaView};

/// The body of a schema request, read for the route that takes it.
// fuzz:parser registry_api
pub(crate) fn read_input(call: &Call<'_>, bound: Bound) -> Result<SchemaInput, RegistryError> {
    body::schema_request(call.body()?, bound)
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

/// `offset` and `limit` on a list, as Confluent applies them to `GET /schemas`: a missing or negative offset is 0; a
/// limit that is not positive is no limit; a value that is not a 32-bit integer is no route (404, as Confluent's
/// framework answers it).
fn page<T>(mut all: Vec<T>, call: &Call<'_>) -> Option<Vec<T>> {
    let num = |name| text::int_param(&call.req.query, name);
    let (offset, limit) = (num("offset")?, num("limit")?);
    if let Some(off) = offset.filter(|&o| o > 0) {
        let off = usize::try_from(off).unwrap_or(usize::MAX).min(all.len());
        all.drain(..off);
    }
    if let Some(lim) = limit.filter(|&l| l > 0) {
        all.truncate(usize::try_from(lim).unwrap_or(usize::MAX));
    }
    Some(all)
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
        // The default context only: contexts are not offered (REG-D3), and Confluent lists this one with none made.
        Route::Contexts => Ok(Response::ok(&["."])),
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
        Route::Versions => Response::ok(&r.versions_where(
            subject,
            call.flag("deleted"),
            call.flag("deletedOnly"),
        )?),
        Route::Register => {
            let input = read_input(call, body::REGISTER)?;
            match r.register(subject, &input, call.flag("normalize"), &call.origin())? {
                RegisterResult::Id(id) => Response::ok(&one("id", id)),
                RegisterResult::Latest(latest) => Response::ok(&SchemaView {
                    subject: String::new(),
                    ..latest
                }),
            }
        }
        Route::Lookup => {
            let input = read_input(call, body::LOOKUP)?;
            Response::ok(&r.lookup(
                subject,
                &input,
                call.flag("normalize"),
                call.flag("deleted"),
            )?)
        }
        Route::DeleteSubject => Response::ok(&r.delete_subject(subject, call.flag("permanent"))?),
        Route::Version => {
            // `format` is not read here: cp-schema-registry 7.7.1 answers the stored text whatever it asks.
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

/// A schema by id, in the order Confluent writes one (`SchemaString`): its type, its text, its references, the highest id.
#[derive(Serialize)]
struct ById {
    #[serde(rename = "schemaType", skip_serializing_if = "Option::is_none")]
    schema_type: Option<crate::format::SchemaType>,
    schema: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    references: Vec<crate::store::Reference>,
    #[serde(rename = "maxId", skip_serializing_if = "Option::is_none")]
    max_id: Option<u32>,
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
            match page(all, call) {
                Some(all) => Response::ok(&all),
                None => Response::error(&RegistryError::http(404, "HTTP 404 Not Found")),
            }
        }
        Route::Types => Response::ok(&r.kinds()),
        Route::SchemaById => {
            let id = schema_id(call)?;
            let view = r.schema_by_id(id)?;
            let format = call.query("format");
            Response::ok(&ById {
                schema_type: view.schema_type,
                schema: r.formatted(id, format.as_deref()).unwrap_or(view.schema),
                references: view.references,
                max_id: call.flag("fetchMaxId").then(|| r.max_id()),
            })
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
    let version = call.at.version.as_deref();
    let bound = if version.is_some() {
        body::COMPAT_VERSION
    } else {
        body::COMPAT_LEVEL
    };
    let input = read_input(call, bound)?;
    let verbose = call.flag("verbose");
    let (_, found) = match r.check(
        call.subject(),
        version,
        &input,
        call.flag("normalize"),
        None,
    ) {
        Ok(found) => found,
        // Verbose, a schema that is not one is an incompatible verdict naming why (recorded by tools/registry-diff).
        Err(e) if verbose && e.error_code == code::INVALID_SCHEMA => {
            return Ok(Response::ok(&Verdict {
                is_compatible: false,
                messages: Some(vec![e.message]),
            }));
        }
        Err(e) => return Err(e),
    };
    // An empty list when it fits, as Confluent sends it.
    let messages = verbose.then(|| {
        found
            .first()
            .map(crate::registry::CompatFinding::messages)
            .unwrap_or_default()
    });
    Ok(Response::ok(&Verdict {
        is_compatible: found.is_empty(),
        messages,
    }))
}

/// A configuration as `GET /config` answers it, in Confluent's order (`Config`).
#[derive(Serialize)]
struct ConfigOut<'a> {
    #[serde(rename = "compatibilityLevel")]
    level: &'a str,
    #[serde(flatten)]
    extra: ExtraOut,
}

/// What a PUT set, as Confluent echoes it (`ConfigUpdateRequest`): only what the request gave, in its order.
#[derive(Serialize)]
struct ConfigEcho {
    #[serde(skip_serializing_if = "Option::is_none")]
    alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    normalize: Option<bool>,
    #[serde(rename = "validateFields", skip_serializing_if = "Option::is_none")]
    validate_fields: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compatibility: Option<String>,
    #[serde(flatten)]
    rest: Groups,
}

/// The settings after the level, in Confluent's order.
#[derive(Serialize)]
struct ExtraOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    normalize: Option<bool>,
    #[serde(rename = "validateFields", skip_serializing_if = "Option::is_none")]
    validate_fields: Option<bool>,
    #[serde(flatten)]
    rest: Groups,
}

/// The group, metadata and rule sets of a configuration, in Confluent's order. A rule set with no rules is not written,
/// as Confluent does not write one.
#[derive(Serialize)]
struct Groups {
    #[serde(rename = "compatibilityGroup", skip_serializing_if = "Option::is_none")]
    compatibility_group: Option<String>,
    #[serde(rename = "defaultMetadata", skip_serializing_if = "Option::is_none")]
    default_metadata: Option<serde_json::Value>,
    #[serde(rename = "overrideMetadata", skip_serializing_if = "Option::is_none")]
    override_metadata: Option<serde_json::Value>,
    #[serde(rename = "defaultRuleSet", skip_serializing_if = "Option::is_none")]
    default_rule_set: Option<serde_json::Value>,
    #[serde(rename = "overrideRuleSet", skip_serializing_if = "Option::is_none")]
    override_rule_set: Option<serde_json::Value>,
}

impl Groups {
    fn of(e: &crate::registry::ConfigExtra) -> Self {
        let json = |t: &Option<String>| t.as_deref().and_then(|t| serde_json::from_str(t).ok());
        let rules = |t: &Option<String>| {
            json(t).filter(|v: &serde_json::Value| {
                v.as_object().is_none_or(|m| {
                    m.values()
                        .any(|r| r.as_array().is_none_or(|a| !a.is_empty()))
                })
            })
        };
        Self {
            compatibility_group: e.compatibility_group.clone(),
            default_metadata: json(&e.default_metadata),
            override_metadata: json(&e.override_metadata),
            default_rule_set: rules(&e.default_rule_set),
            override_rule_set: rules(&e.override_rule_set),
        }
    }
}

fn config_out(c: &ConfigView) -> Response {
    Response::ok(&ConfigOut {
        level: c.level.name(),
        extra: ExtraOut {
            alias: c.extra.alias.clone(),
            normalize: c.extra.normalize,
            validate_fields: c.extra.validate_fields,
            rest: Groups::of(&c.extra),
        },
    })
}

fn config(r: &Registry, route: Route, call: &Call<'_>) -> Result<Response, RegistryError> {
    let subject = call.subject();
    Ok(match route {
        Route::GetConfig => config_out(&r.config("", true)?),
        Route::GetSubjectConfig => config_out(&r.config(subject, call.flag("defaultToGlobal"))?),
        Route::PutConfig | Route::PutSubjectConfig => {
            let bound = if route == Route::PutConfig {
                body::CONFIG
            } else {
                body::SUBJECT_CONFIG
            };
            let req = body::config_request(call.body()?, bound)?;
            let level = req.compatibility.filter(|c| !c.is_empty());
            // As Confluent: nothing to set, nothing changed, `{}`.
            if level.is_some() || !req.extra.is_empty() {
                r.set_config(subject, level.as_deref(), &req.extra)?;
            }
            Response::ok(&ConfigEcho {
                alias: req.extra.alias.clone(),
                normalize: req.extra.normalize,
                validate_fields: req.extra.validate_fields,
                compatibility: level,
                rest: Groups::of(&req.extra),
            })
        }
        Route::DeleteConfig | Route::DeleteSubjectConfig => config_out(&r.delete_config(subject)?),
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
            let bound = if route == Route::PutMode {
                body::MODE
            } else {
                body::SUBJECT_MODE
            };
            let asked = body::mode_request(call.body()?, bound)?.unwrap_or_default();
            r.set_mode(subject, &asked, call.flag("force"))?;
            // The mode is echoed as it was written (`readonly` stays lower case), as Confluent echoes it.
            Response::ok(&one("mode", asked))
        }
        Route::DeleteSubjectMode => Response::ok(&one("mode", r.delete_mode(subject)?.name())),
        _ => return Err(RegistryError::http(404, "HTTP 404 Not Found")),
    })
}
