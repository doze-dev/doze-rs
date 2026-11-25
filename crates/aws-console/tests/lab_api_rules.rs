//! The Lab API's rules: checked in words, made, listed, switched, ended, forgotten, stopped all at once; and the 51st refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod lab_api;
mod support;

use lab_api::{body, lab, throttle};
use serde_json::{Value, json};
use support::{keys, ts_keys};

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
