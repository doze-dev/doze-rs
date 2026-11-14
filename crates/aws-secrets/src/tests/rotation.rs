//! Rotation as the service sees it: the pending version, the run it hands back, completion when AWSCURRENT moves, cancellation.

use serde_json::json;

use super::World;

const FUNCTION: &str = "arn:aws:lambda:us-east-1:000000000000:function:rotator";

fn stages(w: &mut World) -> serde_json::Value {
    w.ok("DescribeSecret", &json!({"SecretId": "db"}))["VersionIdsToStages"].clone()
}

#[test]
fn a_rotation_makes_a_pending_version_and_completes_when_current_moves_to_it() {
    let mut w = World::new();
    w.create("db", "orig");
    let plain = w.ok("DescribeSecret", &json!({"SecretId": "db"}));
    assert!(
        plain.get("RotationEnabled").is_none(),
        "a secret never rotated says nothing of rotation"
    );
    let rotated = w.ok(
        "RotateSecret",
        &json!({"SecretId": "db", "RotationLambdaARN": FUNCTION, "RotationRules": {"AutomaticallyAfterDays": 7}, "ClientRequestToken": "tok-00000000000000000000000000000001"}),
    );
    let token = rotated["VersionId"].as_str().unwrap().to_owned();
    assert_eq!(stages(&mut w)[&token], json!(["AWSPENDING"]));
    // The run is handed to the service to start after the answer is saved.
    let run = w.core.run.take().expect("a run is started");
    assert_eq!(
        (run.function.as_str(), run.token.as_str()),
        (FUNCTION, token.as_str())
    );
    // The pending version has no value until the function puts one.
    let (code, _) = w.err(
        "GetSecretValue",
        &json!({"SecretId": "db", "VersionStage": "AWSPENDING"}),
    );
    assert_eq!(code, "ResourceNotFoundException");
    w.ok(
        "PutSecretValue",
        &json!({"SecretId": "db", "ClientRequestToken": token, "SecretString": "new", "VersionStages": ["AWSPENDING"]}),
    );
    let current = stages(&mut w)
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, s)| s.as_array().unwrap().contains(&json!("AWSCURRENT")))
        .map(|(id, _)| id.clone())
        .unwrap();
    w.ok(
        "UpdateSecretVersionStage",
        &json!({"SecretId": "db", "VersionStage": "AWSCURRENT", "MoveToVersionId": token, "RemoveFromVersionId": current}),
    );
    let md = w.ok("DescribeSecret", &json!({"SecretId": "db"}));
    assert_eq!(md["RotationEnabled"], true);
    assert_eq!(md["RotationLambdaARN"], FUNCTION);
    assert_eq!(md["RotationRules"], json!({"AutomaticallyAfterDays": 7}));
    assert!(md["LastRotatedDate"].is_number() && md["NextRotationDate"].is_number());
    assert_eq!(
        md["VersionIdsToStages"][&token],
        json!(["AWSCURRENT"]),
        "AWSPENDING is removed once the rotation completes"
    );
    assert_eq!(w.get("db")["SecretString"], "new");
    // The list shows the same.
    let listed = w.ok("ListSecrets", &json!({}));
    assert_eq!(listed["SecretList"][0]["RotationEnabled"], true);
}

#[test]
fn an_unfinished_rotation_blocks_the_next_and_cancel_clears_it() {
    let mut w = World::new();
    w.create("db", "orig");
    let (code, message) = w.err("RotateSecret", &json!({"SecretId": "db"}));
    assert_eq!(code, "InvalidRequestException", "{message}");
    w.ok(
        "RotateSecret",
        &json!({"SecretId": "db", "RotationLambdaARN": FUNCTION}),
    );
    assert!(w.core.run.take().is_some());
    let (code, message) = w.err("RotateSecret", &json!({"SecretId": "db"}));
    assert_eq!(
        (code.as_str(), message.as_str()),
        (
            "InvalidRequestException",
            "A previous rotation isn't complete. That rotation will be reattempted."
        )
    );
    let cancelled = w.ok("CancelRotateSecret", &json!({"SecretId": "db"}));
    assert!(cancelled["VersionId"].is_string());
    let md = w.ok("DescribeSecret", &json!({"SecretId": "db"}));
    assert_eq!(md["RotationEnabled"], false);
    assert_eq!(
        md["VersionIdsToStages"].as_object().unwrap().len(),
        1,
        "the empty pending version is gone"
    );
    // Not immediately: the configuration is stored and nothing is started.
    w.ok(
        "RotateSecret",
        &json!({"SecretId": "db", "RotateImmediately": false}),
    );
    assert!(w.core.run.take().is_none());
    assert_eq!(
        w.ok("DescribeSecret", &json!({"SecretId": "db"}))["RotationEnabled"],
        true
    );
}
