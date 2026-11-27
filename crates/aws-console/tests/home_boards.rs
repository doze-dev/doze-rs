//! What the Home boards read beyond the record of apps: the services any call reached and the idle limit that decides which are
//! awake, a service's group, and the subscriptions doze-aws keeps and delivers nothing to.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use std::time::Duration;

use aws_gateway::{Did, Finished};
use aws_peers::Mode;
use serde_json::json;
use support::{keys, ts_keys};
use world::world;

fn wire(key: &'static str, signing: &'static str, action: &'static str) -> Finished<'static> {
    Finished {
        request_id: "req",
        signing_name: signing,
        sdk_id: "SQS",
        action,
        member: "",
        resource: "",
        access_key_id: key,
        region: "us-east-1",
        status: 200,
        code: "",
        took: Duration::from_micros(900),
        would_be_denied: false,
        allowed_by: None,
        user_agent: "",
        message: "",
        function_error: "",
        answer_bytes: None,
        did: Did::NONE,
        lab: None,
    }
}

#[tokio::test]
async fn a_service_any_call_reached_is_active_and_the_idle_limit_is_said() {
    let w = world("activity", Mode::Soft, &[]);
    // An app calls SQS; this console alone calls S3: Home's "apps last called" has SQS, the services awake have both.
    w.state
        .recorder
        .record(&wire("AKIAAPP", "sqs", "SendMessage"), 1);
    w.state
        .recorder
        .record(&wire("console", "s3", "ListBuckets"), 2);
    let (status, home) = w.get("/api/v1/home").await;
    assert_eq!(status, 200, "{home}");
    assert_eq!(keys(&home), ts_keys("HomeView"));
    let apps: Vec<&str> = home["services"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["service"].as_str().unwrap())
        .collect();
    assert_eq!(apps, ["sqs"], "{home}");
    let mut active: Vec<&str> = home["activity"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["service"].as_str().unwrap())
        .collect();
    active.sort_unstable();
    assert_eq!(active, ["s3", "sqs"], "{home}");
    // No `idle-secs` in the run's flags: the setting's default.
    assert_eq!(home["idle_secs"], 300);
}

#[tokio::test]
async fn every_service_says_what_it_is_for() {
    let w = world("groups", Mode::Soft, &[]);
    let (status, services) = w.get("/api/v1/services").await;
    assert_eq!(status, 200);
    let group = |name: &str| {
        services["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == name)
            .map(|s| s["group"].as_str().unwrap().to_owned())
    };
    assert_eq!(group("sqs").as_deref(), Some("Messaging and events"));
    assert_eq!(group("s3").as_deref(), Some("Storage and data"));
    assert_eq!(group("iam").as_deref(), Some("Identity and access"));
    assert_eq!(group("sts").as_deref(), Some("Identity and access"));
}

#[tokio::test]
async fn a_subscription_nothing_is_delivered_to_needs_a_look() {
    let w = world("stored-only", Mode::Soft, &[]);
    w.state
        .recorder
        .record(&wire("AKIAAPP", "sns", "Subscribe"), 1);
    let topic = w
        .app("sns", "CreateTopic", &json!({"Name": "alerts"}))
        .await["TopicArn"]
        .as_str()
        .unwrap()
        .to_owned();
    for (protocol, endpoint) in [("email", "dev@larder.dev"), ("sms", "+15550100")] {
        w.app(
            "sns",
            "Subscribe",
            &json!({"TopicArn": topic, "Protocol": protocol, "Endpoint": endpoint}),
        )
        .await;
    }
    let (status, checks) = w.get("/api/v1/home/checks").await;
    assert_eq!(status, 200, "{checks}");
    assert_eq!(checks["checked"], json!(["sns"]));
    let found = &checks["signals"][0];
    assert_eq!(found["kind"], "not-active");
    assert_eq!(found["count"], 2);
    assert_eq!(found["detail"], "2 subscriptions stored");
}

#[tokio::test]
async fn services_come_in_the_order_of_the_resources_index() {
    let w = world("order", Mode::Soft, &[]);
    let (_, services) = w.get("/api/v1/services").await;
    let names: Vec<&str> = services["services"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    let at = |n: &str| names.iter().position(|x| *x == n).unwrap();
    assert!(
        at("sqs") < at("sns") && at("sns") < at("s3") && at("s3") < at("iam"),
        "{names:?}"
    );
}

#[tokio::test]
async fn a_call_carries_the_words_of_its_error_a_function_error_and_the_length_of_its_answer() {
    let w = world("record-words", Mode::Soft, &[]);
    let refused = Finished {
        status: 400,
        code: "InvalidParameterValue",
        message: "Can only include alphanumeric characters, hyphens, or underscores. 1 to 80 in length",
        answer_bytes: Some(180),
        ..wire("AKIAAPP", "sqs", "CreateQueue")
    };
    let failed = Finished {
        function_error: "Unhandled",
        answer_bytes: Some(73),
        ..wire("AKIAAPP", "lambda", "Invoke")
    };
    w.state.recorder.record(&refused, 1);
    w.state.recorder.record(&failed, 2);
    w.state
        .recorder
        .record(&wire("AKIAAPP", "sqs", "ListQueues"), 3);
    let (status, page) = w.get("/api/v1/calls").await;
    assert_eq!(status, 200, "{page}");
    let calls = page["calls"].as_array().unwrap();
    let by = |op: &str| calls.iter().find(|c| c["action"] == op).unwrap();
    assert_eq!(keys(by("CreateQueue")), ts_keys("CallRecord"));
    assert_eq!(
        by("CreateQueue")["message"],
        "Can only include alphanumeric characters, hyphens, or underscores. 1 to 80 in length"
    );
    assert_eq!(by("CreateQueue")["answer_bytes"], 180);
    // The invocation succeeded (200) and the function failed: the answer says so.
    assert_eq!(by("Invoke")["outcome"], "ok");
    assert_eq!(by("Invoke")["function_error"], "Unhandled");
    // Nothing is made up for a call that carried none of it.
    assert!(by("ListQueues")["message"].is_null());
    assert!(by("ListQueues")["function_error"].is_null());
    assert!(by("ListQueues")["answer_bytes"].is_null());
}

#[tokio::test]
async fn gateway_domain_names_and_vpc_links_are_stored_and_say_so() {
    let w = world("stored-gateway", Mode::Soft, &[]);
    w.state
        .recorder
        .record(&wire("AKIAAPP", "apigateway", "GetRestApis"), 1);
    w.app(
        "apigateway",
        "CreateDomainName",
        &json!({"domainName": "api.larder.dev", "regionalCertificateArn": "arn:aws:acm:us-east-1:000000000000:certificate/c", "endpointConfiguration": {"types": ["REGIONAL"]}}),
    )
    .await;
    let (status, checks) = w.get("/api/v1/home/checks").await;
    assert_eq!(status, 200, "{checks}");
    assert_eq!(checks["checked"], json!(["apigateway"]));
    let found = &checks["signals"][0];
    assert_eq!(found["kind"], "not-active");
    assert_eq!(found["detail"], "1 domain name stored");
    assert_eq!(found["href"], "/resources/apigateway");
}
