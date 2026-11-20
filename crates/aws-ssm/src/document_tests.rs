//! Tests of document content: how it is checked, read and converted.

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
