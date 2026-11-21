//! What the tests of delete with dependents share: reading a blockers answer, and the entities they hang from.
#![allow(dead_code)]

use serde_json::{Value, json};

use crate::world::World;

pub(crate) const BASE: &str = "/api/v1/resources";

pub(crate) fn titles(b: &Value) -> Vec<String> {
    b["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["title"].as_str().unwrap().to_owned())
        .collect()
}

pub(crate) fn names(b: &Value, title: &str) -> Vec<String> {
    b["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["title"] == title)
        .unwrap_or_else(|| panic!("no {title} in {b}"))["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap().to_owned())
        .collect()
}

pub(crate) fn doc() -> String {
    json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": "s3:GetObject", "Resource": "*"}]})
        .to_string()
}

/// An object put the way the console's own upload puts one.
pub(crate) async fn put(w: &World, bucket: &str, key: &str, body: &str) {
    let (status, done) = w
        .send(
            "POST",
            &format!("{BASE}/s3/buckets/{bucket}/object?key={key}"),
            &json!(body),
        )
        .await;
    assert_eq!(status, 201, "{done}");
}

pub(crate) async fn role(w: &World, name: &str) {
    let trust = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow",
        "Principal": {"Service": "lambda.amazonaws.com"}, "Action": "sts:AssumeRole"}]});
    w.app(
        "iam",
        "CreateRole",
        &json!({"RoleName": name, "AssumeRolePolicyDocument": trust.to_string()}),
    )
    .await;
}

pub(crate) async fn policy(w: &World, name: &str) -> String {
    let made = w
        .app(
            "iam",
            "CreatePolicy",
            &json!({"PolicyName": name, "PolicyDocument": doc()}),
        )
        .await;
    made["Policy"]["Arn"].as_str().unwrap().to_owned()
}
