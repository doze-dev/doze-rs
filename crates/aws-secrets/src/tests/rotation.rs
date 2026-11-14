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
    let run = w.core.runs.pop().expect("a run is started");
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
    assert!(w.core.runs.pop().is_some());
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
    // Not immediately: the configuration is stored, and testSecret alone is run against a pending copy of the current value.
    let rotated = w.ok(
        "RotateSecret",
        &json!({"SecretId": "db", "RotateImmediately": false}),
    );
    let run = w.core.runs.pop().expect("a testSecret run");
    assert_eq!(run.step, crate::rotate::Step::TestOnly);
    let token = rotated["VersionId"].as_str().unwrap();
    assert_eq!(stages(&mut w)[token], json!(["AWSPENDING"]));
    assert_eq!(
        w.ok(
            "GetSecretValue",
            &json!({"SecretId": "db", "VersionStage": "AWSPENDING"})
        )["SecretString"],
        "orig"
    );
    let md = w.ok("DescribeSecret", &json!({"SecretId": "db"}));
    assert_eq!(md["RotationEnabled"], true);
    // Not a rotation in progress: another RotateSecret is not refused.
    w.ok("RotateSecret", &json!({"SecretId": "db"}));
}

#[test]
fn the_schedule_starts_a_rotation_when_its_window_opens() {
    let mut w = World::new();
    w.create("db", "orig");
    w.ok(
        "RotateSecret",
        &json!({"SecretId": "db", "RotationLambdaARN": FUNCTION, "RotationRules": {"ScheduleExpression": "rate(4 hours)"}, "RotateImmediately": false}),
    );
    w.core.runs.clear();
    let due = w.core.next_due().expect("a window is due");
    let md = w.ok("DescribeSecret", &json!({"SecretId": "db"}));
    assert_eq!(
        md["NextRotationDate"].as_f64().unwrap() * 1000.0,
        due as f64
    );
    // Before the window nothing starts.
    assert!(w.core.tick().runs.is_empty());
    w.clock.set(due);
    let answer = w.core.tick();
    assert_eq!(answer.runs.len(), 1);
    assert_eq!(answer.runs[0].step, crate::rotate::Step::All);
    assert_eq!(
        answer.next_due, None,
        "nothing is due while the rotation is in progress"
    );
    let (code, _) = w.err("RotateSecret", &json!({"SecretId": "db"}));
    assert_eq!(code, "InvalidRequestException");
}

#[test]
fn a_manual_value_counts_as_a_rotation_for_automatically_after_days() {
    let mut w = World::new();
    w.create("db", "orig");
    w.ok(
        "RotateSecret",
        &json!({"SecretId": "db", "RotationLambdaARN": FUNCTION, "RotationRules": {"AutomaticallyAfterDays": 2}, "RotateImmediately": false}),
    );
    let before = w.core.next_due().unwrap();
    w.clock.set(before - 1);
    w.ok(
        "PutSecretValue",
        &json!({"SecretId": "db", "SecretString": "by hand"}),
    );
    assert!(w.core.next_due().unwrap() > before);
    let (code, _) = w.err(
        "RotateSecret",
        &json!({"SecretId": "db", "RotationRules": {"ScheduleExpression": "rate(1 hour)"}}),
    );
    assert_eq!(code, "InvalidParameterException");
}

/// The service's timer: armed by the call that schedules a rotation, and armed again by `resume` after a restart.
#[tokio::test(flavor = "current_thread")]
async fn the_timer_is_armed_by_the_schedule_and_again_by_resume() {
    use std::sync::Arc;

    use aws_wire::service::Service;
    use doze_runtime::{Ctx, SystemClock, Tasks};

    let dir = super::temp();
    let ctx = |tasks: &Tasks| {
        Arc::new(Ctx {
            dir: dir.path().join("secretsmanager"),
            idle: std::time::Duration::from_secs(300),
            tasks: tasks.clone(),
            clock: Arc::new(SystemClock),
            ext: None,
        })
    };
    let first = Tasks::new();
    let h = aws_testkit::Harness::new(crate::Secrets::new(ctx(&first)));
    let svc = crate::Secrets::new(ctx(&first));
    for (op, body) in [
        ("CreateSecret", json!({"Name": "db"})),
        (
            "RotateSecret",
            json!({"SecretId": "db", "RotationLambdaARN": FUNCTION, "RotationRules": {"AutomaticallyAfterDays": 30}, "RotateImmediately": false}),
        ),
    ] {
        let call = h.call(h.op(op), &body).unwrap();
        svc.call(call).await.unwrap();
    }
    assert!(svc.timer_armed("us-east-1"), "a rotation is scheduled");
    first.shutdown().await;
    drop(svc);
    let second = Tasks::new();
    let again = crate::Secrets::new(ctx(&second));
    assert!(!again.timer_armed("us-east-1"));
    again.resume().await;
    assert!(again.timer_armed("us-east-1"), "resume arms it again");
    second.shutdown().await;
}
