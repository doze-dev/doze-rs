//! The Parameter Store pages' operations: what a parameter's page may make without a resource bound, and what the list filters by,
//! against the real service on one bus.
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

fn names(list: &Value) -> Vec<String> {
    list["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_owned())
        .collect()
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
