//! The Lab's API: rules checked in words, made, listed, switched, ended and forgotten; the 51st refused; the guide; the Lab off; and
//! Live's rows tagged with the rule that touched them and filtered by it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::sync::Arc;

use aws_console::app::console;
use aws_console::lab::LabTable;
use aws_console::state::{Shared, State};
use aws_console::{LabFacts, Limits, Recorder};
use aws_gateway::{Done, Finished, LabNote};
use axum::Router;
use serde_json::{Value, json};
use support::{HOST, get, json as answer, keys, ts_keys};
use tokio_util::sync::CancellationToken;

/// A write as the console's own page sends it.
fn write(method: &str, path: &str, body: &Value) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header(axum::http::header::HOST, HOST)
        .header("x-console", "1")
        .header(axum::http::header::ORIGIN, format!("http://{HOST}"))
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap()
}

struct Lab {
    app: Router,
    state: Shared,
}

fn lab(on: bool) -> Lab {
    let mut facts = support::facts();
    facts.lab = if on {
        LabFacts {
            table: Some(LabTable::new()),
            off_why: None,
            open: false,
        }
    } else {
        LabFacts {
            table: None,
            off_why: Some("The Lab is off: doze-aws was started with --lab=false.".to_owned()),
            open: false,
        }
    };
    let state = State::new(
        facts,
        Arc::new(Recorder::new()),
        HOST.parse().unwrap(),
        Vec::new(),
        CancellationToken::new(),
        &Limits::default(),
    );
    Lab {
        app: console(state.clone()),
        state,
    }
}

impl Lab {
    async fn get(&self, path: &str) -> (u16, Value) {
        answer(&self.app, get(path)).await
    }

    async fn post(&self, path: &str, body: &Value) -> (u16, Value) {
        answer(&self.app, write("POST", path, body)).await
    }
}

fn body(effect: Value) -> Value {
    json!({
        "name": "",
        "matcher": {
            "services": ["sqs"],
            "operations": ["SendMessage"],
            "regions": [],
            "callers": [],
            "resources": ["orders"],
        },
        "effect": effect,
        "share": { "kind": "nth", "n": 4 },
        "ends_in_secs": 600,
    })
}

fn throttle() -> Value {
    body(json!({ "kind": "fail", "error": "throttle" }))
}

#[tokio::test]
async fn at_first_there_are_no_rules_and_the_answer_has_the_keys_of_its_type() {
    let lab = lab(true);
    let (status, v) = lab.get("/api/v1/lab/rules").await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(keys(&v), ts_keys("LabRules"));
    assert_eq!(keys(&v["limits"]), ts_keys("LabLimits"));
    assert_eq!(v["enabled"], true);
    assert_eq!(v["off_why"], Value::Null);
    assert_eq!(
        (
            v["in_force"].as_array().unwrap().len(),
            v["ended"].as_array().unwrap().len()
        ),
        (0, 0)
    );
    assert_eq!(v["limits"]["most_rules"], 50);
    assert_eq!(v["limits"]["most_ended"], 200);
    assert_eq!(v["limits"]["longest_secs"], 86_400);
    assert_eq!(v["limits"]["longest_delay_ms"], 60_000);
}

#[tokio::test]
async fn a_rule_is_checked_in_words_before_it_is_made() {
    let lab = lab(true);
    let (status, v) = lab.post("/api/v1/lab/rules/check", &throttle()).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(keys(&v), ts_keys("LabCheck"));
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(
        v["sentence"],
        "Throttle 1 in 4 SQS SendMessage calls to orders for 10 minutes"
    );
    assert_eq!(
        v["answers"],
        json!([{ "service": "SQS", "status": 403, "code": "RequestThrottled" }])
    );
    // Nothing was made.
    assert_eq!(lab.get("/api/v1/lab/rules").await.1["in_force"], json!([]));
}

#[tokio::test]
async fn what_is_wrong_with_a_rule_is_said_field_by_field() {
    let lab = lab(true);
    let mut bad = throttle();
    bad["matcher"]["services"] = json!(["sqz"]);
    bad["name"] = json!("n".repeat(81));
    bad["share"] = json!({ "kind": "percent", "percent": 100 });
    bad["ends_in_secs"] = json!(0);
    bad["max_hits"] = json!(0);
    let (_, v) = lab.post("/api/v1/lab/rules/check", &bad).await;
    assert_eq!(v["ok"], false);
    let fields: Vec<&str> = v["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["field"].as_str().unwrap())
        .collect();
    for field in ["services", "name", "share", "ends_in_secs", "max_hits"] {
        assert!(fields.contains(&field), "{field} in {fields:?}");
    }
    let text = v["problems"][0]["text"].as_str().unwrap();
    assert!(text.ends_with('.') && text.len() > 20, "{text}");
    // An operation none of the named services has, a delay out of bounds, a custom error that is no error.
    let mut slow = body(json!({ "kind": "slow", "delay_ms": 0, "jitter_ms": 70_000 }));
    slow["matcher"]["operations"] = json!(["SendMesage"]);
    let (_, v) = lab.post("/api/v1/lab/rules/check", &slow).await;
    let said = v["problems"].to_string();
    assert!(said.contains("SendMesage"), "{said}");
    assert!(said.contains("from 1 to 60000 milliseconds"), "{said}");
    let custom =
        body(json!({ "kind": "fail", "error": "custom", "status": 200, "code": "no good" }));
    let (_, v) = lab.post("/api/v1/lab/rules/check", &custom).await;
    assert_eq!(v["problems"].as_array().unwrap().len(), 2, "{v}");
    let no_status = body(json!({ "kind": "fail", "error": "custom", "code": "Teapot" }));
    let (_, v) = lab.post("/api/v1/lab/rules/check", &no_status).await;
    assert_eq!(v["ok"], false);
    // Hostile words are said, not kept, and a body that is not a rule is a 400.
    let mut hostile = throttle();
    hostile["matcher"]["resources"] = json!(["ok", "bad\u{7}", ""]);
    let (_, v) = lab.post("/api/v1/lab/rules/check", &hostile).await;
    assert_eq!(v["ok"], false);
    let (status, v) = lab
        .post("/api/v1/lab/rules/check", &json!({ "nope": 1 }))
        .await;
    assert_eq!((status, v["code"].as_str()), (400, Some("BAD_REQUEST")));
}

#[tokio::test]
async fn a_rule_is_made_listed_switched_ended_and_forgotten() {
    let lab = lab(true);
    let (status, made) = lab.post("/api/v1/lab/rules", &throttle()).await;
    assert_eq!(status, 200, "{made}");
    assert_eq!(made["outcome"], "made");
    let rule = &made["rule"];
    assert_eq!(keys(rule), ts_keys("LabRule"));
    let id = rule["id"].as_u64().unwrap();
    assert_eq!(rule["name"], "Throttle sqs SendMessage");
    assert_eq!(rule["kind"], "fail");
    assert_eq!(rule["share_words"], "1 in 4");
    assert_eq!(rule["what"], "SQS SendMessage");
    assert_eq!(rule["target"], "orders");
    assert_eq!(
        (
            rule["enabled"].clone(),
            rule["hits"].clone(),
            rule["ended"].clone()
        ),
        (json!(true), json!(0), Value::Null)
    );
    assert_eq!(
        rule["effect"],
        json!({ "kind": "fail", "error": "throttle" })
    );

    let (_, listed) = lab.get("/api/v1/lab/rules").await;
    assert_eq!(listed["in_force"][0]["id"], id);

    // Switched off, and back on (with no body it turns the other way).
    let (status, off) = lab
        .post(
            &format!("/api/v1/lab/rules/{id}/switch"),
            &json!({ "enabled": false }),
        )
        .await;
    assert_eq!((status, off["enabled"].clone()), (200, json!(false)));
    let (_, on) = lab
        .post(&format!("/api/v1/lab/rules/{id}/switch"), &json!({}))
        .await;
    assert_eq!(on["enabled"], true);

    // One rule, in force: its page.
    let (status, one) = lab.get(&format!("/api/v1/lab/rules/{id}")).await;
    assert_eq!(status, 200);
    assert_eq!(keys(&one), ts_keys("LabRuleView"));
    assert_eq!(one["calls"], json!([]));

    // Ended by hand, it moves to the list of what ended, and its page is still there.
    let (status, gone) = lab
        .post(&format!("/api/v1/lab/rules/{id}/remove"), &json!({}))
        .await;
    assert_eq!((status, gone["ended"].clone()), (200, json!("cleared")));
    let (_, listed) = lab.get("/api/v1/lab/rules").await;
    assert_eq!(listed["in_force"], json!([]));
    assert_eq!(listed["ended"][0]["id"], id);
    assert_eq!(listed["ended"][0]["ended"], "cleared");
    let (status, one) = lab.get(&format!("/api/v1/lab/rules/{id}")).await;
    assert_eq!(
        (status, one["rule"]["ended"].clone()),
        (200, json!("cleared"))
    );
    // An ended rule cannot be switched or removed again.
    let (status, v) = lab
        .post(&format!("/api/v1/lab/rules/{id}/switch"), &json!({}))
        .await;
    assert_eq!((status, v["code"].as_str()), (409, Some("OVER")));
    let (status, v) = lab
        .post(&format!("/api/v1/lab/rules/{id}/remove"), &json!({}))
        .await;
    assert_eq!((status, v["code"].as_str()), (409, Some("OVER")));

    // The history is forgotten.
    let (status, cleared) = lab.post("/api/v1/lab/history/clear", &json!({})).await;
    assert_eq!((status, cleared["cleared"].clone()), (200, json!(1)));
    assert_eq!(keys(&cleared), ts_keys("LabCleared"));
    let (status, v) = lab.get(&format!("/api/v1/lab/rules/{id}")).await;
    assert_eq!((status, v["code"].as_str()), (404, Some("NOT_FOUND")));
    let (status, v) = lab.get("/api/v1/lab/rules/abc").await;
    assert_eq!((status, v["code"].as_str()), (404, Some("NOT_FOUND")));
}

#[tokio::test]
async fn stop_all_ends_every_rule_and_says_how_many() {
    let lab = lab(true);
    for queue in ["a", "b", "c"] {
        let mut b = throttle();
        b["matcher"]["resources"] = json!([queue]);
        let (_, made) = lab.post("/api/v1/lab/rules", &b).await;
        assert_eq!(made["outcome"], "made", "{made}");
    }
    let (status, stopped) = lab.post("/api/v1/lab/stop-all", &json!({})).await;
    assert_eq!((status, stopped["stopped"].clone()), (200, json!(3)));
    assert_eq!(keys(&stopped), ts_keys("LabStopped"));
    let (_, listed) = lab.get("/api/v1/lab/rules").await;
    assert_eq!(listed["in_force"], json!([]));
    assert_eq!(listed["ended"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn the_51st_rule_is_refused_in_words_and_a_replacement_is_not_counted_twice() {
    let lab = lab(true);
    for n in 0..50 {
        let mut b = throttle();
        b["matcher"]["resources"] = json!([format!("queue-{n}")]);
        let (_, made) = lab.post("/api/v1/lab/rules", &b).await;
        assert_eq!(made["outcome"], "made", "{n}: {made}");
    }
    let mut b = throttle();
    b["matcher"]["resources"] = json!(["one-too-many"]);
    let (status, refused) = lab.post("/api/v1/lab/rules", &b).await;
    assert_eq!(status, 200);
    assert_eq!(refused["outcome"], "refused");
    assert_eq!(keys(&refused["check"]), ts_keys("LabCheck"));
    let problem = &refused["check"]["problems"][0];
    assert_eq!(problem["field"], "rules");
    assert!(
        problem["text"].as_str().unwrap().contains("50 rules"),
        "{problem}"
    );
    let (_, v) = lab.post("/api/v1/lab/rules/check", &b).await;
    assert_eq!(v["ok"], false);
    // The same calls and kind of effect replace the old rule even now.
    let mut again = throttle();
    again["matcher"]["resources"] = json!(["queue-3"]);
    again["effect"] = json!({ "kind": "fail", "error": "internal" });
    let (_, made) = lab.post("/api/v1/lab/rules", &again).await;
    assert_eq!(made["outcome"], "made", "{made}");
    let (_, listed) = lab.get("/api/v1/lab/rules").await;
    assert_eq!(listed["in_force"].as_array().unwrap().len(), 50);
    assert_eq!(listed["ended"][0]["ended"], "replaced");
}

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
