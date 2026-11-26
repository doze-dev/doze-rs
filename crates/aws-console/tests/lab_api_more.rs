//! The Lab API beside its rules: what a Fail rule answers each service with, the Lab off, the guide, the same-origin proof, and Live's rows tagged and filtered by the rule that touched them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod lab_api;
mod support;

use aws_gateway::{Done, Finished, LabNote};
use lab_api::{body, lab, throttle};
use serde_json::{Value, json};
use support::{HOST, json as answer, keys, ts_keys};

#[tokio::test]
async fn a_fail_rule_says_what_each_service_it_names_answers() {
    let lab = lab(true);
    let mut b = body(json!({ "kind": "fail", "error": "throttle" }));
    b["matcher"] =
        json!({ "services": [], "operations": [], "regions": [], "callers": [], "resources": [] });
    let (_, v) = lab.post("/api/v1/lab/rules/check", &b).await;
    let answers = v["answers"].as_array().unwrap();
    assert_eq!(
        answers.len(),
        21,
        "every API of the build, when no service is named"
    );
    let by = |s: &str| answers.iter().find(|a| a["service"] == s).unwrap().clone();
    assert_eq!(
        by("DynamoDB")["code"],
        "ProvisionedThroughputExceededException"
    );
    assert_eq!(
        (by("S3")["status"].clone(), by("S3")["code"].clone()),
        (json!(503), json!("SlowDown"))
    );
    assert_eq!(by("STS")["code"], "ThrottlingException");
    assert_eq!(v["sentence"], "Throttle 1 in 4 calls for 10 minutes");
    let access = body(json!({ "kind": "fail", "error": "access-denied" }));
    let (_, v) = lab.post("/api/v1/lab/rules/check", &access).await;
    assert_eq!(
        v["answers"],
        json!([{ "service": "SQS", "status": 400, "code": "AccessDeniedException" }])
    );
    // A slow rule and a drop have no answers.
    let (_, v) = lab
        .post("/api/v1/lab/rules/check", &body(json!({ "kind": "drop" })))
        .await;
    assert_eq!(v["answers"], json!([]));
}

#[tokio::test]
async fn with_the_lab_off_nothing_can_be_made_and_the_answer_says_why() {
    let lab = lab(false);
    let (status, v) = lab.get("/api/v1/lab/rules").await;
    assert_eq!(status, 200);
    assert_eq!(v["enabled"], false);
    assert!(v["off_why"].as_str().unwrap().contains("--lab=false"));
    assert_eq!(v["in_force"], json!([]));
    assert_eq!(keys(&v), ts_keys("LabRules"));
    let (status, v) = lab.post("/api/v1/lab/rules", &throttle()).await;
    assert_eq!((status, v["code"].as_str()), (409, Some("LAB_OFF")));
    let (status, v) = lab.post("/api/v1/lab/stop-all", &json!({})).await;
    assert_eq!((status, v["code"].as_str()), (409, Some("LAB_OFF")));
    // A check still says what the rule would be, and that it cannot be made.
    let (status, v) = lab.post("/api/v1/lab/rules/check", &throttle()).await;
    assert_eq!(status, 200);
    assert_eq!(v["ok"], false);
    assert!(v["sentence"].as_str().unwrap().starts_with("Throttle"));
    // The words are there to read.
    let (status, v) = lab.get("/api/v1/lab/guide").await;
    assert_eq!(status, 200);
    assert_eq!(v["rehearsals"].as_array().unwrap().len(), 6);
}

#[tokio::test]
async fn the_guide_has_the_intro_and_the_six_lessons_in_the_order_of_the_tabs() {
    let lab = lab(true);
    let (status, v) = lab.get("/api/v1/lab/guide").await;
    assert_eq!(status, 200);
    assert_eq!(keys(&v), ts_keys("LabGuide"));
    assert_eq!(keys(&v["intro"]), ts_keys("LabIntro"));
    assert_eq!(keys(&v["rehearsals"][0]), ts_keys("LabTeach"));
    assert_eq!(
        keys(&v["rehearsals"][0]["settings"][0]),
        ts_keys("LabSetting")
    );
    assert_eq!(keys(&v["intro"]["steps"][0]), ts_keys("LabStep"));
    let tabs: Vec<&str> = v["rehearsals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["tab"].as_str().unwrap())
        .collect();
    assert_eq!(
        tabs,
        [
            "throttle",
            "unavailable",
            "internal",
            "access-denied",
            "slow",
            "drop"
        ]
    );
}

#[tokio::test]
async fn a_write_asks_for_the_same_origin_proof_like_every_write() {
    let lab = lab(true);
    for path in [
        "/api/v1/lab/rules",
        "/api/v1/lab/rules/check",
        "/api/v1/lab/stop-all",
        "/api/v1/lab/history/clear",
        "/api/v1/lab/rules/1/switch",
        "/api/v1/lab/rules/1/remove",
    ] {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri(path)
            .header(axum::http::header::HOST, HOST)
            .header("x-console", "1")
            .header(axum::http::header::ORIGIN, "http://evil.example")
            .body(axum::body::Body::from(throttle().to_string()))
            .unwrap();
        let (status, v) = answer(&lab.app, req).await;
        assert_eq!(
            (status, v["code"].as_str()),
            (403, Some("FORBIDDEN")),
            "{path}"
        );
    }
    assert_eq!(lab.get("/api/v1/lab/rules").await.1["in_force"], json!([]));
}

fn touched(rule: u32, done: Done) -> Finished<'static> {
    Finished {
        lab: Some(LabNote {
            rule,
            name: "ignored: the table names its rules",
            done,
        }),
        ..support::sqs_call("SendMessage", 403, "RequestThrottled")
    }
}

#[tokio::test]
async fn live_tags_the_calls_a_rule_touched_and_filters_by_the_rule() {
    let lab = lab(true);
    let (_, made) = lab.post("/api/v1/lab/rules", &throttle()).await;
    let id = u32::try_from(made["rule"]["id"].as_u64().unwrap()).unwrap();
    let recorder = &lab.state.recorder;
    recorder.record(&support::sqs_call("ListQueues", 200, ""), 1);
    recorder.record(&touched(id, Done::Failed), 2);
    recorder.record(
        &touched(
            id + 40,
            Done::Delayed(std::time::Duration::from_millis(250)),
        ),
        3,
    );
    recorder.record(
        &Finished {
            status: 0,
            code: "",
            ..touched(id, Done::Dropped)
        },
        4,
    );

    let (_, all) = lab.get("/api/v1/calls").await;
    let rows = all["calls"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["lab"], Value::Null);
    assert_eq!(
        rows[1]["lab"],
        json!({ "id": id, "name": "Throttle sqs SendMessage", "effect": { "kind": "failed" } })
    );
    // A rule the table has forgotten still has a number.
    assert_eq!(rows[2]["lab"]["name"], format!("Lab rule {}", id + 40));
    assert_eq!(
        rows[2]["lab"]["effect"],
        json!({ "kind": "delayed", "ms": 250 })
    );
    // A dropped call has no status and is an error; the tag says what was done.
    assert_eq!(
        (rows[3]["status"].clone(), rows[3]["outcome"].clone()),
        (json!(0), json!("error"))
    );
    assert_eq!(rows[3]["lab"]["effect"], json!({ "kind": "dropped" }));

    let (status, mine) = lab.get(&format!("/api/v1/calls?lab={id}")).await;
    assert_eq!(status, 200);
    let seqs: Vec<u64> = mine["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["seq"].as_u64().unwrap())
        .collect();
    assert_eq!(seqs, [2, 4], "oldest first, only this rule's");
    let (_, any) = lab.get("/api/v1/calls?lab=any").await;
    assert_eq!(any["calls"].as_array().unwrap().len(), 3, "the Lab only");
    let (status, v) = lab.get("/api/v1/calls?lab=nope").await;
    assert_eq!((status, v["code"].as_str()), (400, Some("BAD_REQUEST")));

    // The rule's page lists what it touched, newest first, the rows Live has.
    let (_, one) = lab.get(&format!("/api/v1/lab/rules/{id}")).await;
    let seqs: Vec<u64> = one["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["seq"].as_u64().unwrap())
        .collect();
    assert_eq!(seqs, [4, 2]);
    assert_eq!(one["more"], false);
}
