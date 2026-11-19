//! An SSM document: what is kept of it, and how its content is read. Content is checked at the level the model documents: it is
//! JSON or YAML (as `DocumentFormat` says; `TEXT` is taken as it is), at most 64 KB, an object, with a `schemaVersion` the type
//! accepts and the part the schema needs (`mainSteps`, `runtimeConfig`, `sessionType`). Nothing runs a document; its parameters,
//! description and platforms are read out of it for `DescribeDocument`, as AWS reads them.
//!
//! The words of each refusal were not recorded: they are marked (memory).

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The largest content a document may have (the model: "must not exceed 64KB").
const MAX_BYTES: usize = 64 * 1024;
/// The most versions a document keeps (AWS's quota, memory).
pub(crate) const MAX_VERSIONS: usize = 1000;

/// A parameter a document declares.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Param {
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) description: Option<String>,
    pub(crate) default: Option<String>,
}

/// One version of a document.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DocVersion {
    pub(crate) number: i64,
    pub(crate) content: String,
    pub(crate) format: String,
    pub(crate) display_name: Option<String>,
    pub(crate) version_name: Option<String>,
    pub(crate) target_type: Option<String>,
    /// The SHA-256 of the content, as hex.
    pub(crate) hash: String,
    pub(crate) created_ms: i64,
    pub(crate) schema_version: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) params: Vec<Param>,
    pub(crate) platforms: Vec<String>,
    /// The documents it requires: name and version.
    pub(crate) requires: Vec<(String, Option<String>)>,
    /// The review status of a change template's version (`UpdateDocumentMetadata`).
    pub(crate) review: Option<String>,
}

/// A document: its versions, oldest first, and what belongs to the name.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Document {
    pub(crate) kind: String,
    pub(crate) owner: String,
    pub(crate) created_ms: i64,
    pub(crate) versions: Vec<DocVersion>,
    pub(crate) default_version: i64,
    pub(crate) tags: BTreeMap<String, String>,
    /// The accounts it is shared with (`all` when it is public), and the version they see.
    pub(crate) shared: Vec<String>,
    pub(crate) shared_version: Option<String>,
}

impl Document {
    pub(crate) fn latest(&self) -> Option<&DocVersion> {
        self.versions.last()
    }

    pub(crate) fn version(&self, number: i64) -> Option<&DocVersion> {
        self.versions.iter().find(|v| v.number == number)
    }

    /// The version a request names by `DocumentVersion` (`$LATEST`, `$DEFAULT` or a number) or `VersionName`; the default when
    /// it names none.
    pub(crate) fn pick(
        &self,
        version: Option<&str>,
        name: Option<&str>,
    ) -> Result<&DocVersion, ApiError> {
        let by_number = match version {
            None | Some("$DEFAULT") => self.default_version,
            Some("$LATEST") => self.latest().map_or(0, |v| v.number),
            Some(n) => n.parse().map_err(|_| invalid_version())?,
        };
        let found = match (version, name) {
            (None, Some(name)) => self
                .versions
                .iter()
                .find(|v| v.version_name.as_deref() == Some(name)),
            (_, Some(name)) => self
                .version(by_number)
                .filter(|v| v.version_name.as_deref() == Some(name)),
            (_, None) => self.version(by_number),
        };
        found.ok_or_else(invalid_version)
    }
}

pub(crate) fn invalid_version() -> ApiError {
    // (memory)
    ApiError::modeled(
        "InvalidDocumentVersion",
        "The document version is not valid or does not exist.",
    )
}

/// Recorded (test_ssm.py::test_document_lifecycle, DescribeDocument); the other operations are held to the same words (memory).
pub(crate) fn invalid_document(name: &str) -> ApiError {
    ApiError::modeled(
        "InvalidDocument",
        format!("Document with name {name} does not exist."),
    )
}

fn invalid_content(message: impl Into<String>) -> ApiError {
    ApiError::modeled("InvalidDocumentContent", message)
}

/// What the content of a new version says about itself.
#[derive(Debug, Default)]
pub(crate) struct Read {
    pub(crate) schema_version: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) params: Vec<Param>,
    pub(crate) platforms: Vec<String>,
    pub(crate) hash: String,
}

/// The schema versions a document type accepts, and the key its content must have; a type not listed is held to neither.
fn schema_of(kind: &str) -> Option<(&'static [&'static str], Option<&'static str>)> {
    Some(match kind {
        "Command" => (&["1.2", "2.0", "2.2"], None),
        "Automation" | "Automation.ChangeTemplate" => (&["0.3"], Some("mainSteps")),
        "Session" => (&["1.0"], Some("sessionType")),
        "Policy" | "Package" => (&["2.0"], None),
        _ => return None,
    })
}

/// Every schema version some document type takes.
const KNOWN_SCHEMA_VERSIONS: &[&str] = &["0.3", "1.0", "1.2", "2.0", "2.2"];

/// A schema version no document type takes. Recorded (test_ssm.py::test_document_content_is_checked) for JSON: `Document schema
/// version is invalid. Value: 9.9 at Line: 1, Column: 24` for `{"schemaVersion":"9.9","mainSteps":[]}`, where line and column
/// are those of the token after the value (the reader has moved on to it). YAML content gets no position (memory).
fn unknown_schema_version(content: &str, format: &str, sv: &str) -> ApiError {
    let at = if format == "YAML" {
        String::new()
    } else {
        after_schema_version(content)
            .map(|(line, column)| format!(" at Line: {line}, Column: {column}"))
            .unwrap_or_default()
    };
    invalid_content(format!(
        "Document schema version is invalid. Value: {sv}{at}"
    ))
}

/// The line and column (from 1) of the JSON token that follows the value of `schemaVersion`.
fn after_schema_version(content: &str) -> Option<(usize, usize)> {
    let key = content.find("\"schemaVersion\"")? + "\"schemaVersion\"".len();
    let rest = content.get(key..)?.trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let value_len = if let Some(body) = rest.strip_prefix('"') {
        let mut escaped = false;
        let end = body.char_indices().find(|&(_, c)| {
            let close = c == '"' && !escaped;
            escaped = c == '\\' && !escaped;
            close
        })?;
        end.0 + 2
    } else {
        rest.find(|c: char| c == ',' || c == '}' || c.is_whitespace())?
    };
    let after = rest.get(value_len..)?.trim_start();
    let after = after.strip_prefix(',').unwrap_or(after).trim_start();
    let offset = content.len() - after.len();
    let before = content.get(..offset)?;
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count()
        + 1;
    Some((line, column))
}

/// The content as JSON, read as `format` says.
pub(crate) fn parse(content: &str, format: &str) -> Result<Option<Value>, ApiError> {
    match format {
        "TEXT" => Ok(None),
        "YAML" => aws_yaml::parse(content).map(Some).map_err(|e| {
            // (memory)
            invalid_content(format!(
                "The content of the document is not valid YAML (line {}, column {}).",
                e.line, e.column
            ))
        }),
        // Recorded (test_ssm.py::test_document_content_is_checked): "JSON not well-formed. at Line: 1, Column: 2" for "{nope".
        _ => serde_json::from_str(content).map(Some).map_err(|e| {
            invalid_content(format!(
                "JSON not well-formed. at Line: {}, Column: {}",
                e.line(),
                e.column()
            ))
        }),
    }
}

/// Checks the content of a new version of a document of type `kind`, and reads what it says about itself.
pub(crate) fn read(content: &str, format: &str, kind: &str) -> Result<Read, ApiError> {
    if content.len() > MAX_BYTES {
        // (memory)
        return Err(ApiError::modeled(
            "MaxDocumentSizeExceeded",
            "The size limit of a document is 64 KB.",
        ));
    }
    let hash = aws_core::hex::encode(&Sha256::digest(content.as_bytes()));
    let Some(doc) = parse(content, format)? else {
        return Ok(Read {
            hash,
            ..Read::default()
        });
    };
    let Value::Object(map) = &doc else {
        return Err(invalid_content(
            "The content of the document must be an object.",
        ));
    };
    let schema_version = map.get("schemaVersion").map(|v| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    });
    if let Some((allowed, needs)) = schema_of(kind) {
        let Some(sv) = &schema_version else {
            // (memory)
            return Err(invalid_content("Document schemaVersion is missing."));
        };
        if !KNOWN_SCHEMA_VERSIONS.contains(&sv.as_str()) {
            return Err(unknown_schema_version(content, format, sv));
        }
        if !allowed.contains(&sv.as_str()) {
            // (memory)
            return Err(ApiError::modeled(
                "InvalidDocumentSchemaVersion",
                format!(
                    "Document schema version, {sv}, is not supported by the document type {kind}. Supported: {}.",
                    allowed.join(", ")
                ),
            ));
        }
        let needs = needs.unwrap_or(if kind == "Command" && sv == "1.2" {
            "runtimeConfig"
        } else {
            "mainSteps"
        });
        if kind != "Policy" && kind != "Package" && !map.contains_key(needs) {
            // Recorded for mainSteps (test_ssm.py::test_document_content_is_checked); the same form for the others (memory).
            return Err(invalid_content(format!(
                "Missing \"{needs}\" in the document."
            )));
        }
        if kind == "Automation.ChangeTemplate" {
            change_template(map)?;
        }
    }
    Ok(Read {
        schema_version,
        description: map
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        params: params(map.get("parameters")),
        platforms: platforms(kind, map),
        hash,
    })
}

fn params(v: Option<&Value>) -> Vec<Param> {
    let Some(Value::Object(ps)) = v else {
        return Vec::new();
    };
    ps.iter()
        .map(|(name, p)| Param {
            name: name.clone(),
            kind: p
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("String")
                .to_owned(),
            description: p
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned),
            default: p
                .get("default")
                .map(|d| d.as_str().map_or_else(|| d.to_string(), str::to_owned)),
        })
        .collect()
}

/// Recorded 2026-11-19: a change template's content is held to a schema that requires `emergencyChange`, and a `timeoutSeconds`
/// on each step. AWS names the first missing property and where it is.
fn change_template(map: &serde_json::Map<String, Value>) -> Result<(), ApiError> {
    let missing = |what: &str, node: &str| {
        invalid_content(format!(
            "[message: \"object has missing required properties ([\\\"{what}\\\"])\" - parent node: {node}]"
        ))
    };
    if !map.contains_key("emergencyChange") {
        return Err(missing("emergencyChange", "/"));
    }
    let steps = map.get("mainSteps").and_then(Value::as_array);
    if let Some(i) = steps.and_then(|s| {
        s.iter()
            .position(|step| step.get("timeoutSeconds").is_none())
    }) {
        return Err(missing("timeoutSeconds", &format!("/mainSteps/{i}")));
    }
    // Recorded 2026-11-19 (after the missing properties): a change template's steps are approvals only.
    for (i, step) in steps.into_iter().flatten().enumerate() {
        let action = step
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if action != "aws:approve" {
            return Err(invalid_content(format!(
                "[message: \"instance value (\\\"{action}\\\") not found in enum (possible values: [\\\"aws:approve\\\"])\" - parent node: /mainSteps/{i}/action]"
            )));
        }
    }
    Ok(())
}

/// The platforms a document runs on (memory): a Command document whose steps all run shell scripts runs on Linux and macOS, one
/// whose steps all run PowerShell on Windows; every other document on all three.
fn platforms(kind: &str, map: &serde_json::Map<String, Value>) -> Vec<String> {
    let all = || vec!["Windows".to_owned(), "Linux".to_owned(), "MacOS".to_owned()];
    if kind != "Command" {
        return all();
    }
    let actions: Vec<String> = match (map.get("mainSteps"), map.get("runtimeConfig")) {
        (Some(Value::Array(steps)), _) => steps
            .iter()
            .filter_map(|s| s.get("action").and_then(Value::as_str).map(str::to_owned))
            .collect(),
        (_, Some(Value::Object(rc))) => rc.keys().cloned().collect(),
        _ => Vec::new(),
    };
    if !actions.is_empty() && actions.iter().all(|a| a == "aws:runShellScript") {
        vec!["Linux".to_owned(), "MacOS".to_owned()]
    } else if !actions.is_empty() && actions.iter().all(|a| a == "aws:runPowerShellScript") {
        vec!["Windows".to_owned()]
    } else {
        all()
    }
}

/// The content of `v` in `format`: as it was written, or written again in the other of JSON and YAML.
pub(crate) fn content_as(v: &DocVersion, format: Option<&str>) -> String {
    let Some(want) = format.filter(|f| *f != v.format && *f != "TEXT" && v.format != "TEXT") else {
        return v.content.clone();
    };
    let Ok(Some(doc)) = parse(&v.content, &v.format) else {
        return v.content.clone();
    };
    if want == "YAML" {
        aws_yaml::emit(&doc)
    } else {
        serde_json::to_string_pretty(&doc).unwrap_or_else(|_| v.content.clone())
    }
}

/// Whether a document name may be created: the model's reserved prefixes are AWS's own.
pub(crate) fn check_name(name: &str) -> Result<(), ApiError> {
    let lower = name.to_ascii_lowercase();
    if ["aws", "amazon", "amzn"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        // (memory)
        return Err(ApiError::modeled(
            "ValidationException",
            format!(
                "Document name {name} can't begin with aws, amazon or amzn: those prefixes are reserved by AWS."
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const SHELL: &str = r#"{"schemaVersion":"2.2","description":"Say hi","parameters":{"Message":{"type":"String","default":"hi"}},
        "mainSteps":[{"action":"aws:runShellScript","name":"say","inputs":{"runCommand":["echo {{Message}}"]}}]}"#;

    #[test]
    fn content_errors_name_where_they_are_as_aws_does() {
        let message = |c: &str| read(c, "JSON", "Command").unwrap_err().message;
        assert_eq!(
            message("{nope"),
            "JSON not well-formed. at Line: 1, Column: 2"
        );
        assert_eq!(
            message(r#"{"schemaVersion":"9.9","mainSteps":[]}"#),
            "Document schema version is invalid. Value: 9.9 at Line: 1, Column: 24"
        );
        assert_eq!(
            message("{\n  \"schemaVersion\": \"9.9\",\n  \"mainSteps\": []\n}"),
            "Document schema version is invalid. Value: 9.9 at Line: 3, Column: 3"
        );
    }

    #[test]
    fn a_command_document_is_read() {
        let r = read(SHELL, "JSON", "Command").unwrap();
        assert_eq!(r.schema_version.as_deref(), Some("2.2"));
        assert_eq!(r.description.as_deref(), Some("Say hi"));
        assert_eq!(r.params[0].default.as_deref(), Some("hi"));
        assert_eq!(r.platforms, ["Linux", "MacOS"]);
        assert_eq!(r.hash.len(), 64);
    }

    #[test]
    fn yaml_reads_and_converts() {
        let yaml = "schemaVersion: '0.3'\nmainSteps:\n  - name: wait\n    action: aws:sleep\n    inputs:\n      Duration: PT1S\n";
        let r = read(yaml, "YAML", "Automation").unwrap();
        assert_eq!(r.platforms.len(), 3);
        let v = DocVersion {
            number: 1,
            content: yaml.to_owned(),
            format: "YAML".into(),
            display_name: None,
            version_name: None,
            target_type: None,
            hash: r.hash,
            created_ms: 0,
            schema_version: r.schema_version,
            description: None,
            params: Vec::new(),
            platforms: Vec::new(),
            requires: Vec::new(),
            review: None,
        };
        let json: Value = serde_json::from_str(&content_as(&v, Some("JSON"))).unwrap();
        assert_eq!(json["mainSteps"][0]["action"], "aws:sleep");
        assert_eq!(content_as(&v, Some("YAML")), yaml);
    }

    #[test]
    fn bad_content_is_refused_by_name() {
        let code = |c: &str, f: &str, k: &str| read(c, f, k).unwrap_err().code.to_string();
        assert_eq!(code("{nope", "JSON", "Command"), "InvalidDocumentContent");
        assert_eq!(code("[1]", "JSON", "Command"), "InvalidDocumentContent");
        assert_eq!(
            code(r#"{"mainSteps":[]}"#, "JSON", "Command"),
            "InvalidDocumentContent"
        );
        assert_eq!(
            code(
                r#"{"schemaVersion":"9.9","mainSteps":[]}"#,
                "JSON",
                "Command"
            ),
            "InvalidDocumentContent"
        );
        assert_eq!(
            code(r#"{"schemaVersion":"2.2"}"#, "JSON", "Command"),
            "InvalidDocumentContent"
        );
        assert_eq!(
            code(&"x".repeat(MAX_BYTES + 1), "TEXT", "ChangeCalendar"),
            "MaxDocumentSizeExceeded"
        );
        assert!(read("BEGIN:VCALENDAR", "TEXT", "ChangeCalendar").is_ok());
        assert!(check_name("AWS-Mine").is_err() && check_name("my-doc").is_ok());
    }
}
