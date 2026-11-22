//! The KMS, Secrets Manager and Parameter Store pages' operations, kind by kind: what each may make without a resource bound, and
//! what the lists filter by, against the real services on one bus.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use serde_json::{Value, json};
use world::{World, encode, world};

const BASE: &str = "/api/v1/resources";

async fn op(w: &World, kind: &str, name: &str, input: &Value) -> (u16, Value) {
    w.send("POST", &format!("{BASE}/{kind}/op/{name}"), input)
        .await
}

async fn ok(w: &World, kind: &str, name: &str, input: Value) -> Value {
    let (status, out) = op(w, kind, name, &input).await;
    assert_eq!(status, 200, "{kind} {name}: {out}");
    out
}

async fn state(w: &World, id: &str) -> String {
    let (_, one) = w.get(&format!("{BASE}/kms/keys/{id}")).await;
    one["badges"][0]["text"].as_str().unwrap().to_owned()
}

fn names(list: &Value) -> Vec<String> {
    list["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn a_key_is_made_with_its_policy_and_tags_an_alias_is_added_and_the_page_reads_its_rotation()
{
    let w = world("kmsetc-keys", Mode::Soft, &[]);
    let policy = json!({"Version": "2012-10-17", "Id": "key-consolepolicy-3", "Statement": [
        {"Sid": "Enable IAM User Permissions", "Effect": "Allow",
         "Principal": {"AWS": "arn:aws:iam::000000000000:root"}, "Action": "kms:*", "Resource": "*"}]})
    .to_string();
    let made = ok(
        &w,
        "kms/keys",
        "CreateKey",
        json!({"KeySpec": "SYMMETRIC_DEFAULT", "KeyUsage": "ENCRYPT_DECRYPT",
            "Description": "Encrypts report-job output", "Policy": policy,
            "Tags": [{"TagKey": "Project", "TagValue": "larder"}]}),
    )
    .await;
    let id = made["KeyMetadata"]["KeyId"].as_str().unwrap().to_owned();
    ok(
        &w,
        "kms/aliases",
        "CreateAlias",
        json!({"AliasName": "alias/larder-reports", "TargetKeyId": id}),
    )
    .await;

    // The page: the state and the kind of key as badges, the tags it was made with.
    let (_, one) = w.get(&format!("{BASE}/kms/keys/{id}")).await;
    let badges: Vec<&str> = one["badges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["text"].as_str().unwrap())
        .collect();
    assert_eq!(badges, ["Enabled", "Symmetric"]);
    let (_, tags) = w.get(&format!("{BASE}/kms/keys/{id}/tags")).await;
    assert_eq!(tags["tags"], json!([{"key": "Project", "value": "larder"}]));
    let aliases = ok(&w, "kms/aliases", "ListAliases", json!({"KeyId": id})).await;
    assert_eq!(aliases["Aliases"][0]["AliasName"], "alias/larder-reports");

    // The policy is read and written as given.
    let got = ok(&w, "kms/keys", "GetKeyPolicy", json!({"KeyId": id})).await;
    assert_eq!(got["Policy"], policy);
    ok(
        &w,
        "kms/keys",
        "PutKeyPolicy",
        json!({"KeyId": id, "PolicyName": "default", "Policy": policy}),
    )
    .await;
    let (status, refused) = op(
        &w,
        "kms/keys",
        "PutKeyPolicy",
        &json!({"KeyId": id, "PolicyName": "default", "Policy": "not json"}),
    )
    .await;
    assert_eq!(status, 400, "{refused}");
    assert_eq!(refused["code"], "MalformedPolicyDocumentException");

    // Rotation: off, then on with a period, then a rotation on demand that the list of rotations shows.
    let off = ok(&w, "kms/keys", "GetKeyRotationStatus", json!({"KeyId": id})).await;
    assert_eq!(off["KeyRotationEnabled"], false);
    ok(
        &w,
        "kms/keys",
        "EnableKeyRotation",
        json!({"KeyId": id, "RotationPeriodInDays": 180}),
    )
    .await;
    let on = ok(&w, "kms/keys", "GetKeyRotationStatus", json!({"KeyId": id})).await;
    assert_eq!(on["RotationPeriodInDays"], 180);
    ok(&w, "kms/keys", "RotateKeyOnDemand", json!({"KeyId": id})).await;
    let rotations = ok(&w, "kms/keys", "ListKeyRotations", json!({"KeyId": id})).await;
    assert_eq!(rotations["Rotations"][0]["RotationType"], "ON_DEMAND");
    ok(&w, "kms/keys", "DisableKeyRotation", json!({"KeyId": id})).await;
}

#[tokio::test]
async fn the_list_actions_move_a_key_between_enabled_disabled_and_pending_deletion_and_back() {
    let w = world("kmsetc-actions", Mode::Soft, &[]);
    let made = ok(&w, "kms/keys", "CreateKey", json!({})).await;
    let id = made["KeyMetadata"]["KeyId"].as_str().unwrap().to_owned();
    ok(&w, "kms/keys", "DisableKey", json!({"KeyId": id})).await;
    assert_eq!(state(&w, &id).await, "Disabled");
    let scheduled = ok(
        &w,
        "kms/keys",
        "ScheduleKeyDeletion",
        json!({"KeyId": id, "PendingWindowInDays": 7}),
    )
    .await;
    assert_eq!(scheduled["KeyState"], "PendingDeletion");
    assert_eq!(state(&w, &id).await, "Pending deletion");
    // The list says when it goes, in a column of its own.
    let (_, list) = w.get(&format!("{BASE}/kms/keys")).await;
    let at = list["columns"]
        .as_array()
        .unwrap()
        .iter()
        .position(|c| c["title"] == "Deletion date")
        .unwrap();
    assert!(list["rows"][0]["cells"][at].is_string(), "{list}");
    // Enable is refused for a key pending deletion; Cancel brings it back Disabled.
    let (status, refused) = op(&w, "kms/keys", "EnableKey", &json!({"KeyId": id})).await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (400, Some("KMSInvalidStateException"))
    );
    ok(&w, "kms/keys", "CancelKeyDeletion", json!({"KeyId": id})).await;
    assert_eq!(state(&w, &id).await, "Disabled");
    // An operation the kind does not list is not made.
    let (status, out) = op(
        &w,
        "kms/keys",
        "Encrypt",
        &json!({"KeyId": id, "Plaintext": "aGk="}),
    )
    .await;
    assert_eq!((status, out["code"].as_str()), (404, Some("NOT_IN_PAGE")));
}

#[tokio::test]
async fn a_secret_is_made_with_its_tags_and_key_versions_move_labels_and_the_policy_round_trips() {
    let w = world("kmsetc-secrets", Mode::Soft, &[]);
    let made = ok(
        &w,
        "secretsmanager/secrets",
        "CreateSecret",
        json!({"Name": "larder/prod/search-key", "Description": "Search provider API key.",
            "SecretString": "{\"api_key\":\"a\"}",
            "Tags": [{"Key": "app", "Value": "search-indexer"}]}),
    )
    .await;
    assert!(made["VersionId"].is_string(), "{made}");
    let name = "larder/prod/search-key";
    let one = format!("{BASE}/secretsmanager/secrets/{}", encode(name));
    let (_, tags) = w.get(&format!("{one}/tags")).await;
    assert_eq!(
        tags["tags"],
        json!([{"key": "app", "value": "search-indexer"}])
    );

    // A new value is a new version; the one it replaces is AWSPREVIOUS.
    ok(
        &w,
        "secretsmanager/secrets",
        "PutSecretValue",
        json!({"SecretId": name, "SecretString": "{\"api_key\":\"b\"}"}),
    )
    .await;
    let versions = ok(
        &w,
        "secretsmanager/secrets",
        "ListSecretVersionIds",
        json!({"SecretId": name}),
    )
    .await;
    let stages: Vec<Vec<&str>> = versions["Versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            v["VersionStages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(stages.len(), 2);
    assert!(stages.contains(&vec!["AWSCURRENT"]) && stages.contains(&vec!["AWSPREVIOUS"]));
    let value = ok(
        &w,
        "secretsmanager/secrets",
        "GetSecretValue",
        json!({"SecretId": name}),
    )
    .await;
    assert_eq!(value["SecretString"], "{\"api_key\":\"b\"}");
    assert_eq!(value["VersionStages"], json!(["AWSCURRENT"]));

    // The resource policy: none, then put, read and deleted again.
    let policy = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow",
        "Principal": {"AWS": "arn:aws:iam::000000000000:root"},
        "Action": "secretsmanager:GetSecretValue", "Resource": "*"}]})
    .to_string();
    let none = ok(
        &w,
        "secretsmanager/secrets",
        "GetResourcePolicy",
        json!({"SecretId": name}),
    )
    .await;
    assert!(none["ResourcePolicy"].is_null(), "{none}");
    ok(
        &w,
        "secretsmanager/secrets",
        "PutResourcePolicy",
        json!({"SecretId": name, "ResourcePolicy": policy}),
    )
    .await;
    let got = ok(
        &w,
        "secretsmanager/secrets",
        "GetResourcePolicy",
        json!({"SecretId": name}),
    )
    .await;
    assert!(got["ResourcePolicy"].is_string(), "{got}");
    ok(
        &w,
        "secretsmanager/secrets",
        "DeleteResourcePolicy",
        json!({"SecretId": name}),
    )
    .await;
}

#[tokio::test]
async fn the_secrets_list_searches_by_name_word_and_tag_and_shows_scheduled_ones_only_when_asked() {
    let w = world("kmsetc-secrets-list", Mode::Soft, &[]);
    for (name, description, tag) in [
        ("larder/prod/stripe-key", "Stripe live key", "payments"),
        (
            "larder/prod/db-credentials",
            "Postgres credentials",
            "orders",
        ),
        ("other/thing", "Something else", "orders"),
    ] {
        ok(
            &w,
            "secretsmanager/secrets",
            "CreateSecret",
            json!({"Name": name, "Description": description, "SecretString": "x",
                "Tags": [{"Key": "app", "Value": tag}]}),
        )
        .await;
    }
    let by = |q: &str| format!("{BASE}/secretsmanager/secrets?p.filter={}", encode(q));
    let (_, by_name) = w.get(&by("name:larder/prod/")).await;
    assert_eq!(
        names(&by_name),
        ["larder/prod/db-credentials", "larder/prod/stripe-key"]
    );
    let (_, case) = w.get(&by("name:Larder/prod/")).await;
    assert!(names(&case).is_empty(), "the name filter is case-sensitive");
    let (_, by_tag) = w.get(&by("tag-value:payments")).await;
    assert_eq!(names(&by_tag), ["larder/prod/stripe-key"]);
    let (_, by_word) = w.get(&by("all:credentials")).await;
    assert_eq!(names(&by_word), ["larder/prod/db-credentials"]);

    let marked = ok(
        &w,
        "secretsmanager/secrets",
        "DeleteSecret",
        json!({"SecretId": "other/thing", "RecoveryWindowInDays": 30}),
    )
    .await;
    assert!(marked["DeletionDate"].is_number(), "{marked}");
    let (_, plain) = w.get(&format!("{BASE}/secretsmanager/secrets")).await;
    assert_eq!(names(&plain).len(), 2);
    let (_, all) = w
        .get(&format!("{BASE}/secretsmanager/secrets?p.deleted=yes"))
        .await;
    assert_eq!(names(&all).len(), 3);
    // Restoring brings it back to the plain list.
    ok(
        &w,
        "secretsmanager/secrets",
        "RestoreSecret",
        json!({"SecretId": "other/thing"}),
    )
    .await;
    let (_, back) = w.get(&format!("{BASE}/secretsmanager/secrets")).await;
    assert_eq!(names(&back).len(), 3);
}

#[tokio::test]
async fn parameters_are_made_and_overwritten_listed_by_filter_labelled_and_deleted_together() {
    let w = world("kmsetc-ssm", Mode::Soft, &[]);
    for (name, kind) in [
        ("/larder/prod/db/host", "String"),
        ("/larder/staging/db/host", "String"),
        ("/larder/staging/feature/new-checkout", "String"),
    ] {
        ok(
            &w,
            "ssm/parameters",
            "PutParameter",
            json!({"Name": name, "Type": kind, "Value": "v1", "Description": "d",
                "Tags": [{"Key": "app", "Value": "larder"}]}),
        )
        .await;
    }
    // A name that exists is refused unless Overwrite is sent, which makes version 2.
    let (status, refused) = op(
        &w,
        "ssm/parameters",
        "PutParameter",
        &json!({"Name": "/larder/prod/db/host", "Type": "String", "Value": "v2"}),
    )
    .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (400, Some("ParameterAlreadyExists"))
    );
    let again = ok(
        &w,
        "ssm/parameters",
        "PutParameter",
        json!({"Name": "/larder/prod/db/host", "Type": "String", "Value": "v2", "Overwrite": true}),
    )
    .await;
    assert_eq!(again["Version"], 2);
    let history = ok(
        &w,
        "ssm/parameters",
        "GetParameterHistory",
        json!({"Name": "/larder/prod/db/host"}),
    )
    .await;
    assert_eq!(history["Parameters"].as_array().unwrap().len(), 2);

    // Labels: a label sits on one version; it moves when it is put on another.
    let labelled = |version: u32, label: &str| json!({"Name": "/larder/prod/db/host", "ParameterVersion": version, "Labels": [label]});
    ok(
        &w,
        "ssm/parameters",
        "LabelParameterVersion",
        labelled(1, "current"),
    )
    .await;
    ok(
        &w,
        "ssm/parameters",
        "LabelParameterVersion",
        labelled(2, "current"),
    )
    .await;
    let history = ok(
        &w,
        "ssm/parameters",
        "GetParameterHistory",
        json!({"Name": "/larder/prod/db/host"}),
    )
    .await;
    assert_eq!(history["Parameters"][0]["Labels"], json!([]));
    assert_eq!(history["Parameters"][1]["Labels"], json!(["current"]));
    ok(
        &w,
        "ssm/parameters",
        "UnlabelParameterVersion",
        labelled(2, "current"),
    )
    .await;

    // The filter tokens: begins with, contains, equals, and a type.
    let by = |q: &str| format!("{BASE}/ssm/parameters?p.filter={}", encode(q));
    let (_, staging) = w.get(&by("Name:BeginsWith:/larder/staging")).await;
    assert_eq!(
        names(&staging),
        [
            "/larder/staging/db/host",
            "/larder/staging/feature/new-checkout"
        ]
    );
    let (_, host) = w.get(&by("Name:Contains:db/host")).await;
    assert_eq!(names(&host).len(), 2);
    let (_, secure) = w.get(&by("Type:Equals:SecureString")).await;
    assert!(names(&secure).is_empty());
    let (_, listed) = w.get(&format!("{BASE}/ssm/parameters")).await;
    let description = listed["columns"]
        .as_array()
        .unwrap()
        .iter()
        .position(|c| c["title"] == "Description")
        .unwrap();
    assert_eq!(listed["rows"][0]["cells"][description], "d");

    // A parameter's header offers Edit.
    let (_, one) = w
        .get(&format!(
            "{BASE}/ssm/parameters/{}",
            encode("/larder/prod/db/host")
        ))
        .await;
    assert_eq!(one["verbs"][0]["id"], "edit");
    assert_eq!(one["verbs"][0]["label"], "Edit");

    // DeleteParameters takes up to ten names and says which it could not delete.
    let gone = ok(
        &w,
        "ssm/parameters",
        "DeleteParameters",
        json!({"Names": ["/larder/staging/db/host", "/larder/staging/feature/new-checkout", "/nope"]}),
    )
    .await;
    assert_eq!(gone["DeletedParameters"].as_array().unwrap().len(), 2);
    assert_eq!(gone["InvalidParameters"], json!(["/nope"]));
    let (_, left) = w.get(&format!("{BASE}/ssm/parameters")).await;
    assert_eq!(names(&left), ["/larder/prod/db/host"]);
}
