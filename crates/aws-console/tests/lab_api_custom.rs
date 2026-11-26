//! The Lab API's "an error I choose": the errors offered come from the model of the operations a rule fits, and a code the model does
//! not list for them is refused in words, by the server, whatever the form sent.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod lab_api;
mod support;

use lab_api::{body, lab};
use serde_json::{Value, json};
use support::{keys, ts_keys};

/// What an app receives for SQS's `QueueDoesNotExist` (its JSON carries the Query code in `x-amzn-query-error`, which the SDKs read).
const GONE: &str = "AWS.SimpleQueueService.NonExistentQueue";

fn custom(status: Option<u16>, code: &str) -> Value {
    let mut effect = json!({ "kind": "fail", "error": "custom", "code": code });
    if let Some(s) = status {
        effect["status"] = json!(s);
    }
    body(effect)
}

fn codes(v: &Value) -> Vec<&str> {
    v["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["code"].as_str().unwrap())
        .collect()
}

fn problems(v: &Value) -> String {
    v["problems"].to_string()
}

#[tokio::test]
async fn the_errors_offered_are_the_ones_the_model_lists_for_the_operation_with_its_status() {
    let lab = lab(true);
    let (status, v) = lab
        .get("/api/v1/lab/errors?services=sqs&operations=SendMessage")
        .await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(keys(&v), ts_keys("LabErrors"));
    assert_eq!(keys(&v["errors"][0]), ts_keys("LabListedError"));
    assert_eq!(v["why"], Value::Null);
    let all = codes(&v);
    assert!(all.contains(&GONE) && all.contains(&"InvalidMessageContents"));
    assert!(!all.contains(&"ThrottlingException"), "{all:?}");
    let missing = v["errors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["code"] == GONE)
        .unwrap();
    assert_eq!(
        (&missing["status"], &missing["server"]),
        (&json!(400), &json!(false))
    );
    // The console's names for a service are read as the matcher reads them.
    let (_, by_sdk_id) = lab
        .get("/api/v1/lab/errors?services=SQS&operations=SendMessage")
        .await;
    assert_eq!(by_sdk_id, v);
}

#[tokio::test]
async fn several_operations_offer_what_they_share_and_nothing_is_offered_without_them() {
    let lab = lab(true);
    let (_, one) = lab
        .get("/api/v1/lab/errors?services=sqs&operations=SendMessage")
        .await;
    let (_, both) = lab
        .get("/api/v1/lab/errors?services=sqs&operations=SendMessage%2CReceiveMessage")
        .await;
    assert!(codes(&both).contains(&GONE));
    assert!(
        !codes(&both).contains(&"InvalidMessageContents"),
        "only SendMessage lists it"
    );
    assert!(codes(&both).len() < codes(&one).len());
    for (query, said) in [
        ("operations=SendMessage", "name the service"),
        ("services=sqs", "name the operation"),
        (
            "services=sqs&operations=SendMesage",
            "lists no error for SendMesage",
        ),
    ] {
        let (status, v) = lab.get(&format!("/api/v1/lab/errors?{query}")).await;
        assert_eq!(status, 200, "{v}");
        assert!(codes(&v).is_empty(), "{query}: {v}");
        assert!(v["why"].as_str().unwrap().contains(said), "{query}: {v}");
    }
}

#[tokio::test]
async fn a_listed_error_makes_a_rule_with_the_models_status_and_the_sentence_and_name_say_the_code()
{
    let lab = lab(true);
    // The status is the model's, so the form may leave it out.
    let (_, check) = lab
        .post(
            "/api/v1/lab/rules/check",
            &custom(None, "QueueDoesNotExist"),
        )
        .await;
    assert_eq!(check["ok"], true, "{check}");
    let sentence = check["sentence"].as_str().unwrap();
    assert!(
        sentence.starts_with(
            "Answer AWS.SimpleQueueService.NonExistentQueue (400) to 1 in 4 SQS SendMessage calls to orders"
        ),
        "{sentence}"
    );
    assert_eq!(check["answers"][0]["code"], GONE);
    let (status, made) = lab
        .post("/api/v1/lab/rules", &custom(Some(400), "QueueDoesNotExist"))
        .await;
    assert_eq!((status, &made["outcome"]), (200, &json!("made")), "{made}");
    let rule = &made["rule"];
    assert_eq!(
        rule["name"],
        "Fail with AWS.SimpleQueueService.NonExistentQueue sqs SendMessage"
    );
    assert_eq!(
        rule["effect"],
        json!({ "kind": "fail", "error": "custom", "status": 400, "code": "AWS.SimpleQueueService.NonExistentQueue" })
    );
    assert_eq!(keys(rule), ts_keys("LabRule"));
    // A code is also taken by the model's shape name, and is made the code a client reads.
    let (_, again) = lab
        .post(
            "/api/v1/lab/rules/check",
            &custom(Some(400), "RequestThrottled"),
        )
        .await;
    assert_eq!(
        again["ok"], false,
        "the model gives RequestThrottled 403: {again}"
    );
    assert!(problems(&again).contains("403, not 400"), "{again}");
}

#[tokio::test]
async fn a_code_the_model_does_not_list_for_the_operation_is_refused_in_words_and_no_rule_is_made()
{
    let lab = lab(true);
    for (code, status) in [
        ("Teapot", Some(418)),
        ("ThrottlingException", None),
        ("InvalidMessageContent", Some(400)),
    ] {
        let (_, check) = lab
            .post("/api/v1/lab/rules/check", &custom(status, code))
            .await;
        assert_eq!(check["ok"], false, "{code}: {check}");
        let said = problems(&check);
        assert!(
            said.contains(&format!("does not list {code} as an error of SendMessage")),
            "{code}: {said}"
        );
        assert_eq!(check["problems"][0]["field"], "effect");
        let (status, made) = lab.post("/api/v1/lab/rules", &custom(status, code)).await;
        assert_eq!(
            (status, &made["outcome"]),
            (200, &json!("refused")),
            "{code}"
        );
    }
    let (_, listed) = lab.get("/api/v1/lab/rules").await;
    assert_eq!(
        listed["in_force"].as_array().unwrap().len(),
        0,
        "nothing was made"
    );
}

#[tokio::test]
async fn a_rule_that_fits_several_operations_needs_a_code_every_one_of_them_lists() {
    let lab = lab(true);
    let mut two = custom(Some(400), "InvalidMessageContents");
    two["matcher"]["operations"] = json!(["SendMessage", "ReceiveMessage"]);
    let (_, check) = lab.post("/api/v1/lab/rules/check", &two).await;
    assert_eq!(check["ok"], false, "{check}");
    assert!(
        problems(&check).contains("every one of ReceiveMessage and SendMessage"),
        "{check}"
    );
    let mut shared = custom(None, "QueueDoesNotExist");
    shared["matcher"]["operations"] = json!(["SendMessage", "ReceiveMessage"]);
    let (_, ok) = lab.post("/api/v1/lab/rules/check", &shared).await;
    assert_eq!(ok["ok"], true, "{ok}");
}

#[tokio::test]
async fn an_error_of_ones_choosing_needs_a_service_and_an_operation_to_choose_from() {
    let lab = lab(true);
    let mut no_service = custom(Some(400), "QueueDoesNotExist");
    no_service["matcher"]["services"] = json!([]);
    let (_, v) = lab.post("/api/v1/lab/rules/check", &no_service).await;
    assert_eq!(v["problems"][0]["field"], "effect", "{v}");
    assert!(problems(&v).contains("name the service first"), "{v}");
    let mut no_operation = custom(Some(400), "QueueDoesNotExist");
    no_operation["matcher"]["operations"] = json!([]);
    let (_, v) = lab.post("/api/v1/lab/rules/check", &no_operation).await;
    assert_eq!(v["problems"][0]["field"], "effect", "{v}");
    assert!(problems(&v).contains("name the operation first"), "{v}");
    assert_eq!(
        v["problems"].as_array().unwrap().len(),
        1,
        "no second complaint about the code: {v}"
    );
    // An operation that is not there is said once, as for every rule, and the model is not asked about it.
    let mut unknown = custom(Some(400), "QueueDoesNotExist");
    unknown["matcher"]["operations"] = json!(["SendMesage"]);
    let (_, v) = lab.post("/api/v1/lab/rules/check", &unknown).await;
    assert_eq!(v["problems"].as_array().unwrap().len(), 1, "{v}");
    // No code at all, and a code that is no code, are the form's to say and are not "not listed".
    let (_, v) = lab.post("/api/v1/lab/rules/check", &custom(None, "")).await;
    assert_eq!(v["ok"], false);
    let (_, v) = lab
        .post("/api/v1/lab/rules/check", &custom(None, "no good"))
        .await;
    assert!(!problems(&v).contains("does not list"), "{v}");
}
