//! The Lab's forced events, from the index to resting a service: what each kind is, what it will not do, and a service that is rested in
//! a region answering its next call and being shown as resting.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;
#[path = "lab_once/world.rs"]
mod world;

use serde_json::{Value, json};
use support::{keys, ts_keys};
use world::{ONCE, lab, sent, write};

#[tokio::test]
async fn the_index_lists_the_five_kinds_and_says_why_one_cannot_be_carried_out() {
    let l = lab("index", &["sqs", "scheduler", "dynamodb", "kms"]);
    let (status, index) = l.get(ONCE).await;
    assert_eq!(status, 200, "{index}");
    assert_eq!(keys(&index), ts_keys("LabOnceIndex"));
    let kinds = index["kinds"].as_array().unwrap();
    let ids: Vec<&str> = kinds.iter().map(|k| k["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        [
            "rest-service",
            "fire-schedule",
            "expire-visibility",
            "sweep-ttl",
            "cold-start"
        ]
    );
    assert_eq!(keys(&kinds[0]), ts_keys("LabOnceKind"));
    let available: Vec<bool> = kinds.iter().map(|k| k["available"] == true).collect();
    assert_eq!(available, [true, true, true, true, false]);
    assert_eq!(
        kinds[4]["why_not"],
        "Lambda is not served in this run (--services), so there is nothing of it to force."
    );
    assert_eq!(kinds[0]["why_not"], Value::Null);
    assert_eq!(index["history"], json!([]));
    // A kind that cannot be carried out says so in words and does nothing.
    let (status, done) = l.force("cold-start", &json!({"choice": "f"})).await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(done["ok"], false);
    assert!(sent(&done["sentence"]).starts_with("Lambda is not served in this run"));
}

#[tokio::test]
async fn an_unknown_kind_a_missing_choice_and_a_region_that_is_not_aws_s_are_refused() {
    let l = lab("refused", &[]);
    let (status, e) = l.get(&format!("{ONCE}/dance/form")).await;
    assert_eq!((status, e["code"].as_str()), (404, Some("NOT_FOUND")));
    let (status, e) = l.force("dance", &json!({"choice": "x"})).await;
    assert_eq!((status, e["code"].as_str()), (404, Some("NOT_FOUND")));
    let (status, e) = l.force("expire-visibility", &json!({})).await;
    assert_eq!((status, e["code"].as_str()), (400, Some("BAD_REQUEST")));
    assert_eq!(e["message"], "Choose a queue.");
    let (status, e) = l
        .force(
            "expire-visibility",
            &json!({"choice": "q", "region": "not-a-region-9"}),
        )
        .await;
    assert_eq!((status, e["code"].as_str()), (400, Some("UNKNOWN_REGION")));
    let (status, e) = l
        .get(&format!("{ONCE}/rest-service/form?region=moon-1"))
        .await;
    assert_eq!((status, e["code"].as_str()), (400, Some("UNKNOWN_REGION")));
    // A refusal of a request that is not one is not an attempt: nothing is kept.
    let (_, index) = l.get(ONCE).await;
    assert_eq!(index["history"], json!([]));
}

#[tokio::test]
async fn the_guide_teaches_each_kind_once_in_the_order_of_the_cards() {
    let l = lab("guide", &[]);
    let (status, guide) = l.get(&format!("{ONCE}/guide")).await;
    assert_eq!(status, 200, "{guide}");
    assert_eq!(keys(&guide), ts_keys("LabOnceGuide"));
    let kinds = guide["kinds"].as_array().unwrap();
    assert_eq!(keys(&kinds[0]), ts_keys("LabOnceTeach"));
    assert_eq!(keys(&kinds[2]["settings"][0]), ts_keys("LabOnceSetting"));
    let taught: Vec<&str> = kinds.iter().map(|k| k["kind"].as_str().unwrap()).collect();
    let (_, index) = l.get(ONCE).await;
    let ids: Vec<&str> = index["kinds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["id"].as_str().unwrap())
        .collect();
    assert_eq!(taught, ids);
}

#[tokio::test]
async fn the_rest_form_says_which_services_run_without_calling_any() {
    let l = lab("rest-form", &[]);
    l.app(
        "us-east-1",
        "sqs",
        "CreateQueue",
        &json!({"QueueName": "q"}),
    )
    .await;
    let before = l.live().len();
    let (status, form) = l.get(&format!("{ONCE}/rest-service/form")).await;
    assert_eq!(status, 200, "{form}");
    assert_eq!(keys(&form), ts_keys("LabOnceForm"));
    assert_eq!(keys(&form["choices"][0]), ts_keys("LabOnceChoice"));
    assert_eq!(keys(&form["option"]), ts_keys("LabOnceOption"));
    let state = |name: &str| {
        form["choices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["value"] == name)
            .map(|c| (c["state"].clone(), c["detail"].clone()))
            .unwrap_or_else(|| panic!("{name} is not offered: {form}"))
    };
    assert_eq!(state("sqs").0, "running");
    assert_eq!(
        state("kms"),
        (
            json!("resting"),
            json!("Not opened in us-east-1 in this run.")
        )
    );
    assert_eq!(form["choices"][0]["value"], "sqs", "the running ones first");
    assert_eq!(l.live().len(), before, "drawing the list called nothing");
    assert!(
        form["preview"]
            .as_str()
            .unwrap()
            .starts_with("Close SQS in us-east-1 now")
    );
    // The same form for another region: nothing runs there.
    let (_, ireland) = l
        .get(&format!(
            "{ONCE}/rest-service/form?region=eu-west-1&choice=sqs"
        ))
        .await;
    assert_eq!(ireland["region"], "eu-west-1");
    assert!(
        ireland["choices"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["state"] == "resting")
    );
    assert!(
        ireland["preview"]
            .as_str()
            .unwrap()
            .starts_with("SQS is already resting in eu-west-1")
    );
}

#[tokio::test]
async fn a_rested_service_answers_its_next_call_keeps_its_data_and_is_shown_as_resting() {
    let l = lab("rest-act", &[]);
    l.app(
        "us-east-1",
        "sqs",
        "CreateQueue",
        &json!({"QueueName": "kept"}),
    )
    .await;
    assert_eq!(l.runs("sqs", "us-east-1"), Some(true));

    let (status, done) = l.force("rest-service", &json!({"choice": "sqs"})).await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(keys(&done), ts_keys("LabOnceDone"));
    assert_eq!(done["ok"], true, "{done}");
    assert!(sent(&done["sentence"]).starts_with(
        "Rested SQS in us-east-1: it closed as it does after 5 minutes without a call"
    ));
    assert_eq!(done["link"], "/live?console=shown&service=sqs");
    assert_eq!(l.runs("sqs", "us-east-1"), Some(false));
    // Home and the Resources index say it rests, not that it runs because it was called a moment ago.
    let (_, index) = l.get("/api/v1/resources?counts=awake").await;
    let sqs = index["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "sqs")
        .unwrap();
    assert_eq!(sqs["asleep"], true, "{sqs}");

    // Its next call opens it again, with its data.
    let listed = l.app("us-east-1", "sqs", "ListQueues", &json!({})).await;
    assert_eq!(
        listed["QueueUrls"][0],
        "http://127.0.0.1:4566/000000000000/kept"
    );
    assert_eq!(l.runs("sqs", "us-east-1"), Some(true));
    let (_, index) = l.get("/api/v1/resources?counts=awake").await;
    let sqs = index["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "sqs")
        .unwrap();
    assert_eq!(sqs["asleep"], false, "a call after the rest woke it");

    // Resting what rests does nothing, and says so.
    l.force("rest-service", &json!({"choice": "sqs"})).await;
    let (_, again) = l.force("rest-service", &json!({"choice": "sqs"})).await;
    assert_eq!(again["ok"], false);
    assert!(
        sent(&again["sentence"]).starts_with(
            "SQS is already resting in us-east-1: it holds no memory and no open files."
        )
    );
    // Every attempt is kept, newest first.
    let (_, index) = l.get(ONCE).await;
    let history = index["history"].as_array().unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(keys(&history[0]), ts_keys("LabOnce"));
    assert_eq!(
        (history[0]["ok"].clone(), history[1]["ok"].clone()),
        (json!(false), json!(true))
    );
    assert_eq!(history[0]["kind"], "rest-service");
    assert_eq!(history[0]["subject"], "sqs");
    assert_eq!(history[0]["region"], "us-east-1");
}

#[tokio::test]
async fn the_wake_is_timed_when_asked_and_the_calls_are_on_live_as_the_consoles() {
    let l = lab("rest-time", &[]);
    l.app(
        "us-east-1",
        "sqs",
        "CreateQueue",
        &json!({"QueueName": "q"}),
    )
    .await;
    let (_, done) = l
        .force("rest-service", &json!({"choice": "sqs", "time_wake": true}))
        .await;
    assert_eq!(done["ok"], true, "{done}");
    let said = sent(&done["sentence"]);
    assert!(
        said.contains("The first call after it (ListQueues) took "),
        "{said}"
    );
    assert!(
        said.contains(", which included opening the service; the same call again took "),
        "{said}"
    );
    let live = l.live();
    let timed = live
        .iter()
        .filter(|c| c.as_str() == "sqs ListQueues")
        .count();
    assert_eq!(timed, 2, "{live:?}");
}

#[tokio::test]
async fn a_region_is_rested_alone() {
    let l = lab("rest-region", &[]);
    for region in ["us-east-1", "eu-west-1"] {
        l.app(region, "sqs", "CreateQueue", &json!({"QueueName": "q"}))
            .await;
    }
    // By the header, as the page's region switch sends it.
    let (status, done) = support::json(
        &l.app,
        write(
            &format!("{ONCE}/rest-service"),
            &json!({"choice": "sqs"}),
            Some("eu-west-1"),
        ),
    )
    .await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(done["ok"], true, "{done}");
    assert!(sent(&done["sentence"]).starts_with("Rested SQS in eu-west-1"));
    assert_eq!(l.runs("sqs", "eu-west-1"), Some(false));
    assert_eq!(
        l.runs("sqs", "us-east-1"),
        Some(true),
        "the other region keeps running"
    );
    // By the body.
    let (_, done) = l
        .force(
            "rest-service",
            &json!({"choice": "sqs", "region": "us-east-1"}),
        )
        .await;
    assert!(sent(&done["sentence"]).starts_with("Rested SQS in us-east-1"));
    assert_eq!(l.runs("sqs", "us-east-1"), Some(false));
    let (_, index) = l.get(ONCE).await;
    let regions: Vec<&str> = index["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["region"].as_str().unwrap())
        .collect();
    assert_eq!(regions, ["us-east-1", "eu-west-1"]);
}

#[tokio::test]
async fn the_history_keeps_two_hundred_and_is_cleared_on_request() {
    let l = lab("history", &[]);
    for _ in 0..205 {
        l.state
            .once
            .add("rest-service", "x", true, "sqs", "us-east-1");
    }
    let (_, index) = l.get(ONCE).await;
    let history = index["history"].as_array().unwrap();
    assert_eq!(history.len(), 200);
    assert_eq!(history[0]["id"], 205, "newest first");
    assert_eq!(history[199]["id"], 6, "the five oldest are gone");
    let (status, cleared) = support::json(
        &l.app,
        write(&format!("{ONCE}/history/clear"), &json!({}), None),
    )
    .await;
    assert_eq!(status, 200, "{cleared}");
    assert_eq!(cleared["history"], json!([]));
    let (_, index) = l.get(ONCE).await;
    assert_eq!(index["history"], json!([]));
}

#[tokio::test]
async fn a_write_without_the_same_origin_proof_is_refused() {
    let l = lab("proof", &[]);
    let mut req = write(
        &format!("{ONCE}/rest-service"),
        &json!({"choice": "sqs"}),
        None,
    );
    req.headers_mut()
        .insert("origin", "http://evil.example".parse().unwrap());
    let (status, e) = support::json(&l.app, req).await;
    assert_eq!((status, e["code"].as_str()), (403, Some("FORBIDDEN")));
    assert_eq!(l.runs("sqs", "us-east-1"), None, "nothing was done");
}
