//! A secret's life, with the answers AWS gave when the conformance scenarios were recorded where there were any.

use serde_json::json;

use super::{START, World};

#[test]
fn a_secret_has_versions_and_moves_its_labels() {
    let mut w = World::new();
    let created = w.create("app/db", "s3cret");
    let first = created["VersionId"].as_str().unwrap().to_owned();
    assert!(
        created["ARN"]
            .as_str()
            .unwrap()
            .starts_with("arn:aws:secretsmanager:us-east-1:000000000000:secret:app/db-")
    );
    let got = w.get("app/db");
    assert_eq!(
        (got["SecretString"].as_str(), got["VersionStages"].clone()),
        (Some("s3cret"), json!(["AWSCURRENT"]))
    );

    let put = w.ok(
        "PutSecretValue",
        &json!({"SecretId": "app/db", "SecretString": "rotated"}),
    );
    assert_eq!(put["VersionStages"], json!(["AWSCURRENT"]));
    assert_eq!(w.get("app/db")["SecretString"], "rotated");
    let previous = w.ok(
        "GetSecretValue",
        &json!({"SecretId": "app/db", "VersionStage": "AWSPREVIOUS"}),
    );
    assert_eq!(
        (
            previous["SecretString"].as_str(),
            previous["VersionId"].as_str()
        ),
        (Some("s3cret"), Some(first.as_str()))
    );

    let described = w.ok("DescribeSecret", &json!({"SecretId": "app/db"}));
    let stages = &described["VersionIdsToStages"];
    assert_eq!(stages[first.as_str()], json!(["AWSPREVIOUS"]));
    assert!(described.get("Tags").is_none(), "no tags, no key");
    assert!(described.get("LastAccessedDate").is_some(), "it was read");
    let versions = w.ok("ListSecretVersionIds", &json!({"SecretId": "app/db"}));
    assert_eq!(versions["Versions"].as_array().unwrap().len(), 2);
    assert_eq!(
        versions["Versions"][0]["KmsKeyIds"],
        json!(["DefaultEncryptionKey"])
    );
}

#[test]
fn the_secret_can_be_found_by_name_or_arn() {
    let mut w = World::new();
    let arn = w.create("by/arn", "v")["ARN"].as_str().unwrap().to_owned();
    assert_eq!(w.get(&arn)["SecretString"], "v");
    let partial = arn.rsplit_once('-').unwrap().0.to_owned();
    assert_eq!(w.get(&partial)["SecretString"], "v");
    assert_eq!(
        w.err(
            "GetSecretValue",
            &json!({"SecretId": format!("{partial}x-AbCdEf")})
        )
        .0,
        "ResourceNotFoundException"
    );
}

#[test]
fn a_binary_secret_comes_back_as_binary() {
    let mut w = World::new();
    w.ok(
        "CreateSecret",
        &json!({"Name": "bin", "SecretBinary": "AAH+"}),
    );
    let got = w.get("bin");
    assert_eq!(got["SecretBinary"], "AAH+");
    assert!(got.get("SecretString").is_none());
}

#[test]
fn a_deleted_secret_waits_out_its_window_and_can_be_restored() {
    let mut w = World::new();
    w.create("gone", "v");
    let del = w.ok(
        "DeleteSecret",
        &json!({"SecretId": "gone", "RecoveryWindowInDays": 7}),
    );
    assert_eq!(
        del["DeletionDate"].as_f64(),
        Some((START as f64 + 7.0 * 86_400_000.0) / 1000.0)
    );
    assert_eq!(
        w.err("GetSecretValue", &json!({"SecretId": "gone"})).0,
        "InvalidRequestException"
    );
    let described = w.ok("DescribeSecret", &json!({"SecretId": "gone"}));
    assert!(described.get("DeletedDate").is_some() && described.get("LastAccessedDate").is_none());
    assert_eq!(w.ok("ListSecrets", &json!({}))["SecretList"], json!([]));
    assert_eq!(
        w.ok("ListSecrets", &json!({"IncludePlannedDeletion": true}))["SecretList"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    w.ok("RestoreSecret", &json!({"SecretId": "gone"}));
    assert_eq!(w.get("gone")["SecretString"], "v");

    w.ok(
        "DeleteSecret",
        &json!({"SecretId": "gone", "RecoveryWindowInDays": 7}),
    );
    w.clock.set(START + 8 * 86_400_000);
    assert_eq!(
        w.err("DescribeSecret", &json!({"SecretId": "gone"})).0,
        "ResourceNotFoundException",
        "the window ran out"
    );
    assert_eq!(w.create("gone", "again")["Name"], "gone");

    w.ok(
        "DeleteSecret",
        &json!({"SecretId": "gone", "ForceDeleteWithoutRecovery": true}),
    );
    assert_eq!(
        w.err("DescribeSecret", &json!({"SecretId": "gone"})).0,
        "ResourceNotFoundException"
    );
}

#[test]
fn tags_descriptions_and_a_client_token() {
    let mut w = World::new();
    let token = "11111111-2222-4333-8444-555555555555";
    let first = w.ok("CreateSecret", &json!({"Name": "t", "SecretString": "v", "ClientRequestToken": token, "Tags": [{"Key": "env", "Value": "dev"}]}));
    assert_eq!(first["VersionId"], token);
    // Asking again with the same token and value is the same version; another value under it is refused.
    let again = w.ok(
        "PutSecretValue",
        &json!({"SecretId": "t", "SecretString": "v", "ClientRequestToken": token}),
    );
    assert_eq!(again["VersionId"], token);
    assert_eq!(
        w.err(
            "PutSecretValue",
            &json!({"SecretId": "t", "SecretString": "other", "ClientRequestToken": token})
        )
        .0,
        "ResourceExistsException"
    );
    w.ok(
        "UpdateSecret",
        &json!({"SecretId": "t", "Description": "for tests"}),
    );
    w.ok(
        "TagResource",
        &json!({"SecretId": "t", "Tags": [{"Key": "team", "Value": "x"}]}),
    );
    w.ok(
        "UntagResource",
        &json!({"SecretId": "t", "TagKeys": ["env"]}),
    );
    let d = w.ok("DescribeSecret", &json!({"SecretId": "t"}));
    assert_eq!(
        (d["Description"].as_str(), d["Tags"].clone()),
        (Some("for tests"), json!([{"Key": "team", "Value": "x"}]))
    );
}

#[test]
fn a_label_moves_with_update_secret_version_stage() {
    let mut w = World::new();
    let one = w.create("s", "1")["VersionId"].as_str().unwrap().to_owned();
    let two = w.ok(
        "PutSecretValue",
        &json!({"SecretId": "s", "SecretString": "2", "VersionStages": ["PENDING"]}),
    )["VersionId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        w.get("s")["VersionId"],
        one.as_str(),
        "a pending version does not take AWSCURRENT"
    );
    w.ok("UpdateSecretVersionStage", &json!({"SecretId": "s", "VersionStage": "AWSCURRENT", "MoveToVersionId": two, "RemoveFromVersionId": one}));
    assert_eq!(w.get("s")["VersionId"], two.as_str());
    let previous = w.ok(
        "GetSecretValue",
        &json!({"SecretId": "s", "VersionStage": "AWSPREVIOUS"}),
    );
    assert_eq!(previous["VersionId"], one.as_str());
    assert_eq!(
        w.err(
            "UpdateSecretVersionStage",
            &json!({"SecretId": "s", "VersionStage": "AWSCURRENT", "RemoveFromVersionId": two})
        )
        .0,
        "InvalidParameterException"
    );
}

#[test]
fn a_version_without_a_label_goes_after_a_day() {
    let mut w = World::new();
    let one = w.create("s", "1")["VersionId"].as_str().unwrap().to_owned();
    w.ok(
        "PutSecretValue",
        &json!({"SecretId": "s", "SecretString": "2"}),
    );
    w.ok(
        "PutSecretValue",
        &json!({"SecretId": "s", "SecretString": "3"}),
    );
    // The first lost AWSPREVIOUS to the second: it has no label now.
    assert!(
        w.ok(
            "ListSecretVersionIds",
            &json!({"SecretId": "s", "IncludeDeprecated": true})
        )["Versions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["VersionId"] == one.as_str())
    );
    w.clock.set(START + 2 * 86_400_000);
    w.ok(
        "PutSecretValue",
        &json!({"SecretId": "s", "SecretString": "4"}),
    );
    assert!(
        !w.ok(
            "ListSecretVersionIds",
            &json!({"SecretId": "s", "IncludeDeprecated": true})
        )["Versions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["VersionId"] == one.as_str())
    );
}

#[test]
fn listing_filters_sorts_and_pages() {
    let mut w = World::new();
    for n in ["a/1", "a/2", "b/1"] {
        w.create(n, "v");
    }
    w.ok(
        "UpdateSecret",
        &json!({"SecretId": "b/1", "Description": "the Blue one"}),
    );
    let names = |v: &serde_json::Value| -> Vec<String> {
        v["SecretList"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["Name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        names(&w.ok(
            "ListSecrets",
            &json!({"Filters": [{"Key": "name", "Values": ["a/"]}]})
        )),
        ["a/1", "a/2"]
    );
    assert_eq!(
        names(&w.ok(
            "ListSecrets",
            &json!({"Filters": [{"Key": "description", "Values": ["blue"]}]})
        )),
        ["b/1"]
    );
    assert_eq!(
        names(&w.ok(
            "ListSecrets",
            &json!({"Filters": [{"Key": "name", "Values": ["!a/"]}]})
        )),
        ["b/1"]
    );
    assert_eq!(
        names(&w.ok("ListSecrets", &json!({"SortOrder": "desc"}))),
        ["b/1", "a/2", "a/1"]
    );
    let page = w.ok("ListSecrets", &json!({"MaxResults": 2}));
    let rest = w.ok(
        "ListSecrets",
        &json!({"MaxResults": 2, "NextToken": page["NextToken"]}),
    );
    assert_eq!(
        (names(&page), names(&rest)),
        (
            vec!["a/1".to_owned(), "a/2".to_owned()],
            vec!["b/1".to_owned()]
        )
    );
    assert!(rest.get("NextToken").is_none());
}

#[test]
fn batch_get_names_what_it_could_not_find() {
    let mut w = World::new();
    w.create("here", "v");
    let got = w.ok(
        "BatchGetSecretValue",
        &json!({"SecretIdList": ["here", "absent"]}),
    );
    assert_eq!(got["SecretValues"][0]["SecretString"], "v");
    assert_eq!(got["Errors"][0]["SecretId"], "absent");
    assert_eq!(got["Errors"][0]["ErrorCode"], "ResourceNotFoundException");
}

#[test]
fn resource_policies_are_kept_and_random_passwords_follow_their_rules() {
    let mut w = World::new();
    w.create("p", "v");
    let policy = r#"{"Version":"2012-10-17","Statement":[]}"#;
    w.ok(
        "PutResourcePolicy",
        &json!({"SecretId": "p", "ResourcePolicy": policy}),
    );
    assert_eq!(
        w.ok("GetResourcePolicy", &json!({"SecretId": "p"}))["ResourcePolicy"],
        policy
    );
    assert_eq!(
        w.err(
            "PutResourcePolicy",
            &json!({"SecretId": "p", "ResourcePolicy": "nope"})
        )
        .0,
        "MalformedPolicyDocumentException"
    );
    w.ok("DeleteResourcePolicy", &json!({"SecretId": "p"}));
    assert!(
        w.ok("GetResourcePolicy", &json!({"SecretId": "p"}))
            .get("ResourcePolicy")
            .is_none()
    );
    assert_eq!(
        w.ok("ValidateResourcePolicy", &json!({"ResourcePolicy": policy}))["PolicyValidationPassed"],
        true
    );

    let pw = w.ok(
        "GetRandomPassword",
        &json!({"PasswordLength": 20, "ExcludeNumbers": true, "ExcludePunctuation": true}),
    )["RandomPassword"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(pw.len(), 20);
    assert!(
        pw.chars().all(|c| c.is_ascii_alphabetic())
            && pw.chars().any(|c| c.is_ascii_uppercase())
            && pw.chars().any(|c| c.is_ascii_lowercase())
    );
    assert_eq!(
        w.ok("GetRandomPassword", &json!({}))["RandomPassword"]
            .as_str()
            .unwrap()
            .len(),
        32
    );
}
