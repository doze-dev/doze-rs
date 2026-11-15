//! SSM documents, service settings and public parameters: what each operation answers, what it refuses, what survives a restart,
//! and what `doze-aws data` sees of them.

use aws_wire::storage::{Emptied, Prune};
use serde_json::{Value, json};

use super::persistence::Run;
use super::{World, temp};
use crate::storage::{DataAsk, DataReply};

const SHELL: &str = r#"{"schemaVersion":"2.2","description":"Say hi","parameters":{"Message":{"type":"String","default":"hi"}},"mainSteps":[{"action":"aws:runShellScript","name":"say","inputs":{"runCommand":["echo {{Message}}"]}}]}"#;

fn content(n: u32) -> String {
    SHELL.replace("echo", &format!("echo {n}"))
}

fn create(w: &mut World, name: &str) -> Value {
    w.ok(
        "CreateDocument",
        &json!({"Name": name, "Content": SHELL, "DocumentType": "Command",
        "Tags": [{"Key": "team", "Value": "a"}]}),
    )
}

#[test]
fn a_document_is_created_read_and_described() {
    let mut w = World::new();
    let made = create(&mut w, "say-hi")["DocumentDescription"].clone();
    assert_eq!(made["Status"], "Creating");
    assert_eq!(made["DocumentVersion"], "1");
    assert_eq!(made["PlatformTypes"], json!(["Linux", "MacOS"]));
    assert_eq!(made["Parameters"][0]["DefaultValue"], "hi");
    assert_eq!(made["HashType"], "Sha256");
    let d = w.ok("DescribeDocument", &json!({"Name": "say-hi"}))["Document"].clone();
    assert_eq!(
        (
            d["Status"].as_str(),
            d["SchemaVersion"].as_str(),
            d["Owner"].as_str()
        ),
        (Some("Active"), Some("2.2"), Some("000000000000"))
    );
    let got = w.ok("GetDocument", &json!({"Name": "say-hi"}));
    assert_eq!(got["Content"], SHELL);
    let yaml = w.ok(
        "GetDocument",
        &json!({"Name": "say-hi", "DocumentFormat": "YAML"}),
    );
    assert_eq!(yaml["DocumentFormat"], "YAML");
    assert!(
        yaml["Content"]
            .as_str()
            .unwrap()
            .contains("\"schemaVersion\": \"2.2\"")
    );
    let by_arn = "arn:aws:ssm:us-east-1:000000000000:document/say-hi";
    assert_eq!(
        w.ok("GetDocument", &json!({"Name": by_arn}))["Name"],
        "say-hi"
    );
    assert_eq!(
        w.err(
            "CreateDocument",
            &json!({"Name": "say-hi", "Content": SHELL})
        )
        .0,
        "DocumentAlreadyExists"
    );
    assert_eq!(
        w.err("GetDocument", &json!({"Name": "absent"})).0,
        "InvalidDocument"
    );
}

#[test]
fn versions_update_and_the_default_moves_only_when_asked() {
    let mut w = World::new();
    create(&mut w, "doc");
    let up = w.ok("UpdateDocument", &json!({"Name": "doc", "Content": content(2), "DocumentVersion": "$LATEST", "VersionName": "two"}));
    assert_eq!(up["DocumentDescription"]["DocumentVersion"], "2");
    assert_eq!(up["DocumentDescription"]["DefaultVersion"], "1");
    assert_eq!(
        w.err(
            "UpdateDocument",
            &json!({"Name": "doc", "Content": content(2)})
        )
        .0,
        "DuplicateDocumentContent"
    );
    assert_eq!(
        w.err(
            "UpdateDocument",
            &json!({"Name": "doc", "Content": content(3), "VersionName": "two"})
        )
        .0,
        "DuplicateDocumentVersionName"
    );
    assert_eq!(
        w.err(
            "UpdateDocument",
            &json!({"Name": "doc", "Content": content(3), "DocumentVersion": "1"})
        )
        .0,
        "InvalidDocumentVersion"
    );
    assert_eq!(
        w.ok("GetDocument", &json!({"Name": "doc"}))["DocumentVersion"],
        "1"
    );
    assert_eq!(
        w.ok("GetDocument", &json!({"Name": "doc", "VersionName": "two"}))["DocumentVersion"],
        "2"
    );
}

#[test]
fn the_default_moves_and_a_version_is_deleted() {
    let mut w = World::new();
    create(&mut w, "doc");
    w.ok(
        "UpdateDocument",
        &json!({"Name": "doc", "Content": content(2), "VersionName": "two"}),
    );
    let set = w.ok(
        "UpdateDocumentDefaultVersion",
        &json!({"Name": "doc", "DocumentVersion": "2"}),
    );
    assert_eq!(
        set["Description"],
        json!({"Name": "doc", "DefaultVersion": "2", "DefaultVersionName": "two"})
    );
    assert_eq!(
        w.err(
            "UpdateDocumentDefaultVersion",
            &json!({"Name": "doc", "DocumentVersion": "9"})
        )
        .0,
        "InvalidDocumentVersion"
    );
    let versions = w.ok(
        "ListDocumentVersions",
        &json!({"Name": "doc", "MaxResults": 1}),
    );
    assert_eq!(versions["DocumentVersions"][0]["IsDefaultVersion"], false);
    let next = w.ok(
        "ListDocumentVersions",
        &json!({"Name": "doc", "NextToken": versions["NextToken"]}),
    );
    assert_eq!(next["DocumentVersions"][0]["IsDefaultVersion"], true);
    assert_eq!(
        w.err(
            "DeleteDocument",
            &json!({"Name": "doc", "DocumentVersion": "2"})
        )
        .0,
        "InvalidDocumentOperation"
    );
    w.ok(
        "DeleteDocument",
        &json!({"Name": "doc", "DocumentVersion": "1"}),
    );
    assert_eq!(
        w.err(
            "GetDocument",
            &json!({"Name": "doc", "DocumentVersion": "1"})
        )
        .0,
        "InvalidDocumentVersion"
    );
    w.ok("DeleteDocument", &json!({"Name": "doc"}));
    assert_eq!(
        w.err("DescribeDocument", &json!({"Name": "doc"})).0,
        "InvalidDocument"
    );
}

#[test]
fn content_and_names_are_checked() {
    let mut w = World::new();
    let bad = |c: &str, t: &str| json!({"Name": "x-doc", "Content": c, "DocumentType": t});
    assert_eq!(
        w.err("CreateDocument", &bad("{", "Command")).0,
        "InvalidDocumentContent"
    );
    assert_eq!(
        w.err(
            "CreateDocument",
            &bad(r#"{"schemaVersion":"0.3","mainSteps":[]}"#, "Command")
        )
        .0,
        "InvalidDocumentSchemaVersion"
    );
    assert_eq!(
        w.err(
            "CreateDocument",
            &json!({"Name": "AWS-Mine", "Content": SHELL})
        )
        .0,
        "ValidationException"
    );
    let yaml = "schemaVersion: '0.3'\nmainSteps:\n  - name: wait\n    action: aws:sleep\n    inputs:\n      Duration: PT1S\n";
    w.ok("CreateDocument", &json!({"Name": "auto", "Content": yaml, "DocumentType": "Automation", "DocumentFormat": "YAML"}));
    let json_back = w.ok(
        "GetDocument",
        &json!({"Name": "auto", "DocumentFormat": "JSON"}),
    );
    let v: Value = serde_json::from_str(json_back["Content"].as_str().unwrap()).unwrap();
    assert_eq!(v["mainSteps"][0]["action"], "aws:sleep");
}

#[test]
fn documents_are_listed_tagged_and_shared() {
    let mut w = World::new();
    create(&mut w, "one");
    w.ok("CreateDocument", &json!({"Name": "two", "Content": r#"{"schemaVersion":"0.3","mainSteps":[]}"#, "DocumentType": "Automation"}));
    let all = w.ok("ListDocuments", &json!({}));
    assert_eq!(all["DocumentIdentifiers"].as_array().unwrap().len(), 2);
    let mine = w.ok("ListDocuments", &json!({"Filters": [{"Key": "Owner", "Values": ["Self"]}, {"Key": "DocumentType", "Values": ["Command"]}]}));
    assert_eq!(mine["DocumentIdentifiers"][0]["Name"], "one");
    let amazon = w.ok(
        "ListDocuments",
        &json!({"Filters": [{"Key": "Owner", "Values": ["Amazon"]}]}),
    );
    assert_eq!(amazon["DocumentIdentifiers"], json!([]));
    let tagged = w.ok(
        "ListDocuments",
        &json!({"Filters": [{"Key": "tag:team", "Values": ["a"]}]}),
    );
    assert_eq!(tagged["DocumentIdentifiers"].as_array().unwrap().len(), 1);
    w.ok("AddTagsToResource", &json!({"ResourceType": "Document", "ResourceId": "two", "Tags": [{"Key": "k", "Value": "v"}]}));
    assert_eq!(
        w.ok(
            "ListTagsForResource",
            &json!({"ResourceType": "Document", "ResourceId": "two"})
        )["TagList"],
        json!([{"Key": "k", "Value": "v"}])
    );
    w.ok("ModifyDocumentPermission", &json!({"Name": "one", "PermissionType": "Share", "AccountIdsToAdd": ["111122223333", "All"]}));
    let perm = w.ok(
        "DescribeDocumentPermission",
        &json!({"Name": "one", "PermissionType": "Share"}),
    );
    assert_eq!(perm["AccountIds"], json!(["111122223333", "all"]));
    assert_eq!(
        perm["AccountSharingInfoList"][0]["SharedDocumentVersion"],
        "$DEFAULT"
    );
    w.ok(
        "ModifyDocumentPermission",
        &json!({"Name": "one", "PermissionType": "Share", "AccountIdsToRemove": ["all"]}),
    );
    w.ok("UpdateServiceSetting", &json!({"SettingId": "/ssm/documents/console/public-sharing-permission", "SettingValue": "Disable"}));
    assert_eq!(
        w.err(
            "ModifyDocumentPermission",
            &json!({"Name": "one", "PermissionType": "Share", "AccountIdsToAdd": ["all"]})
        )
        .0,
        "InvalidPermissionType"
    );
    assert_eq!(
        w.err(
            "UpdateDocumentMetadata",
            &json!({"Name": "one", "DocumentReviews": {"Action": "Approve"}})
        )
        .0,
        "InvalidDocumentOperation"
    );
}

#[test]
fn service_settings_hold_defaults_until_updated() {
    let mut w = World::new();
    let id = "/ssm/parameter-store/high-throughput-enabled";
    let got = w.ok("GetServiceSetting", &json!({"SettingId": id}))["ServiceSetting"].clone();
    assert_eq!(
        (got["SettingValue"].as_str(), got["Status"].as_str()),
        (Some("false"), Some("Default"))
    );
    assert_eq!(
        got["ARN"],
        format!("arn:aws:ssm:us-east-1:000000000000:servicesetting{id}")
    );
    w.ok(
        "UpdateServiceSetting",
        &json!({"SettingId": got["ARN"], "SettingValue": "true"}),
    );
    let now = w.ok("GetServiceSetting", &json!({"SettingId": id}))["ServiceSetting"].clone();
    assert_eq!(
        (now["SettingValue"].as_str(), now["Status"].as_str()),
        (Some("true"), Some("Customized"))
    );
    let reset = w.ok("ResetServiceSetting", &json!({"SettingId": id}))["ServiceSetting"].clone();
    assert_eq!(reset["SettingValue"], "false");
    assert_eq!(
        w.err(
            "UpdateServiceSetting",
            &json!({"SettingId": id, "SettingValue": "maybe"})
        )
        .0,
        "ValidationException"
    );
    assert_eq!(
        w.err("GetServiceSetting", &json!({"SettingId": "/ssm/nothing"}))
            .0,
        "ServiceSettingNotFound"
    );
    assert_eq!(
        w.err(
            "GetServiceSetting",
            &json!({"SettingId": "/ssm/managed-instance/default-ec2-instance-management-role"})
        )
        .0,
        "ServiceSettingNotFound"
    );
    // The default tier is the tier of a parameter written without one.
    w.ok("UpdateServiceSetting", &json!({"SettingId": "/ssm/parameter-store/default-parameter-tier", "SettingValue": "Advanced"}));
    assert_eq!(w.put("/tiered", "v")["Tier"], "Advanced");
}

#[test]
fn public_parameters_are_read_and_never_written() {
    let mut w = World::new();
    let name = "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64";
    let p = w.ok("GetParameter", &json!({"Name": name}))["Parameter"].clone();
    assert!(p["Value"].as_str().unwrap().starts_with("ami-0"));
    assert_eq!(p["ARN"], format!("arn:aws:ssm:us-east-1::parameter{name}"));
    assert_eq!(
        w.ok("GetParameter", &json!({"Name": format!("{name}:1")}))["Parameter"]["Selector"],
        ":1"
    );
    assert_eq!(
        w.err("GetParameter", &json!({"Name": format!("{name}:2")}))
            .0,
        "ParameterVersionNotFound"
    );
    assert_eq!(
        w.err("GetParameter", &json!({"Name": "/aws/service/absent"}))
            .0,
        "ParameterNotFound"
    );
    let many = w.ok(
        "GetParameters",
        &json!({"Names": [name, "/aws/service/absent"]}),
    );
    assert_eq!(many["Parameters"].as_array().unwrap().len(), 1);
    assert_eq!(many["InvalidParameters"], json!(["/aws/service/absent"]));
    let path = "/aws/service/global-infrastructure/services";
    let page = w.ok(
        "GetParametersByPath",
        &json!({"Path": path, "MaxResults": 5}),
    );
    assert_eq!(page["Parameters"].as_array().unwrap().len(), 5);
    let rest = w.ok(
        "GetParametersByPath",
        &json!({"Path": path, "NextToken": page["NextToken"], "MaxResults": 10}),
    );
    assert_eq!(rest["Parameters"][0]["Name"], format!("{path}/ecs"));
    let deep = w.ok("GetParametersByPath", &json!({"Path": "/aws/service/ami-amazon-linux-latest", "Recursive": true, "MaxResults": 10}));
    assert!(deep["NextToken"].is_string());
    assert_eq!(
        w.ok("DescribeParameters", &json!({}))["Parameters"],
        json!([])
    );
    assert_eq!(
        w.err(
            "PutParameter",
            &json!({"Name": name, "Value": "ami-x", "Type": "String", "Overwrite": true})
        )
        .0,
        "AccessDeniedException"
    );
}

#[test]
fn documents_and_settings_survive_a_restart() {
    let dir = temp();
    let mut r = Run::new(dir.path());
    r.step(
        "CreateDocument",
        &json!({"Name": "doc-d", "Content": SHELL, "Tags": [{"Key": "k", "Value": "v"}]}),
    );
    r.step(
        "UpdateDocument",
        &json!({"Name": "doc-d", "Content": content(2)}),
    );
    r.step(
        "UpdateDocumentDefaultVersion",
        &json!({"Name": "doc-d", "DocumentVersion": "2"}),
    );
    r.step(
        "ModifyDocumentPermission",
        &json!({"Name": "doc-d", "PermissionType": "Share", "AccountIdsToAdd": ["111122223333"]}),
    );
    r.step(
        "RemoveTagsFromResource",
        &json!({"ResourceType": "Document", "ResourceId": "doc-d", "TagKeys": ["k"]}),
    );
    r.step(
        "UpdateServiceSetting",
        &json!({"SettingId": "/ssm/opsinsights/opscenter", "SettingValue": "Enabled"}),
    );
    r.step("GetDocument", &json!({"Name": "doc-d"}));
    r.step(
        "ResetServiceSetting",
        &json!({"SettingId": "/ssm/opsinsights/opscenter"}),
    );
    r.step(
        "DeleteDocument",
        &json!({"Name": "doc-d", "DocumentVersion": "1"}),
    );
    r.step("DeleteDocument", &json!({"Name": "doc-d"}));
}

fn emptied(w: &mut World, ask: DataAsk) -> Emptied {
    match w.core.data("us-east-1", ask) {
        DataReply::Emptied(done) => done.unwrap(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn data_sees_document_versions_and_a_reset_removes_documents_and_settings() {
    let mut w = World::new();
    create(&mut w, "doc-d");
    for n in 2..=4 {
        w.ok(
            "UpdateDocument",
            &json!({"Name": "doc-d", "Content": content(n)}),
        );
    }
    w.ok(
        "UpdateServiceSetting",
        &json!({"SettingId": "/ssm/opsinsights/opscenter", "SettingValue": "Enabled"}),
    );
    let DataReply::Usage(resources, _) = w.core.data("us-east-1", DataAsk::Usage) else {
        panic!("no usage");
    };
    let doc = resources
        .iter()
        .find(|r| r.name == "document/doc-d")
        .unwrap();
    assert_eq!(doc.items, 4);
    assert_eq!(
        doc.notes.len(),
        1,
        "versions 2 and 3 are neither default nor latest"
    );
    let prune = Prune {
        old_versions: true,
        ..Prune::default()
    };
    let done = emptied(
        &mut w,
        DataAsk::Empty {
            name: "document/doc-d".into(),
            prune,
        },
    );
    assert_eq!(done.items, 2);
    let versions = w.ok("ListDocumentVersions", &json!({"Name": "doc-d"}));
    assert_eq!(versions["DocumentVersions"].as_array().unwrap().len(), 2);
    let reset = emptied(&mut w, DataAsk::Reset { dry_run: false });
    assert_eq!(reset.items, 3, "two versions and one setting");
    assert_eq!(
        w.err("GetDocument", &json!({"Name": "doc-d"})).0,
        "InvalidDocument"
    );
    let s = w.ok(
        "GetServiceSetting",
        &json!({"SettingId": "/ssm/opsinsights/opscenter"}),
    );
    assert_eq!(s["ServiceSetting"]["Status"], "Default");
}
