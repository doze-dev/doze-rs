//! The first endpoints, each answered by the router: what this doze-aws is, how much of each service is built, and the Live
//! record as pages. The keys of each answer are the keys the generated TypeScript type declares.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use aws_gateway::Finished;
use support::{app, get, json, keys, sqs_call, state, state_of, ts_keys};

#[tokio::test]
async fn about_says_the_console_is_where_it_was_advertised_when_it_was() {
    let mut facts = support::facts();
    facts.console_url = "http://local.shop.doze:4567".into();
    let app = app(&state_of(facts, 8, &[]));
    let (status, about) = json(&app, get("/api/v1/about")).await;
    assert_eq!(status, 200, "{about}");
    assert_eq!(about["console"], "http://local.shop.doze:4567");
}

#[tokio::test]
async fn about_says_what_this_doze_aws_is() {
    let s = state(8, &[]);
    let app = app(&s);
    let (status, about) = json(&app, get("/api/v1/about")).await;
    assert_eq!(status, 200, "{about}");
    assert_eq!(keys(&about), ts_keys("About"));
    assert_eq!(about["name"], "doze-aws");
    assert_eq!(about["version"], "9.9.9");
    assert_eq!(about["account"], "000000000000");
    assert_eq!(about["region"], "us-east-1");
    assert_eq!(about["endpoint"], "http://127.0.0.1:4566");
    assert_eq!(about["console"], "http://127.0.0.1:4567");
    assert_eq!(
        (&about["auth"], &about["iam"]),
        (&"trust".into(), &"soft".into())
    );
    assert_eq!(about["data_dir"], "/tmp/doze-aws-data");
    assert_eq!(
        (&about["pid"], &about["started"]),
        (&42.into(), &1_800_000_000_u64.into())
    );
    let services = about["services"].as_array().unwrap();
    assert_eq!(services.len(), 2);
    assert_eq!(keys(&services[0]), ts_keys("ServiceName"));
    assert_eq!(
        services[0],
        serde_json::json!({"name": "s3", "title": "S3", "served": false})
    );
    assert_eq!(services[1]["served"], true);
    // The regions are the default and whatever calls were signed for.
    assert_eq!(about["regions"], serde_json::json!(["us-east-1"]));
    s.recorder.record(
        &Finished {
            region: "eu-west-1",
            ..sqs_call("ListQueues", 200, "")
        },
        1,
    );
    let (_, about) = json(&app, get("/api/v1/about")).await;
    assert_eq!(
        about["regions"],
        serde_json::json!(["eu-west-1", "us-east-1"])
    );
}

#[tokio::test]
async fn services_counts_each_api_s_operations_on_the_service_itself() {
    let s = state(8, &[]);
    let (status, body) = json(&app(&s), get("/api/v1/services")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(keys(&body), ts_keys("ServicesResponse"));
    let rows = body["services"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    // In the Resources index's order: SQS before S3.
    let (sqs, s3) = (&rows[0], &rows[1]);
    assert_eq!(keys(sqs), ts_keys("ServiceRow"));
    assert_eq!(
        (&s3["name"], &s3["built"], &s3["total"]),
        (&"s3".into(), &0.into(), &0.into())
    );
    assert_eq!(
        (&sqs["name"], &sqs["title"], &sqs["served"]),
        (&"sqs".into(), &"SQS".into(), &true.into())
    );
    // What `doze-aws compat` would say of this service: every operation of the model, and which the service serves.
    let service = &s.facts.services[1].apis[0];
    let total = (0..)
        .take_while(|i| service.tables().op(*i).is_some())
        .count();
    let built = (0..u32::try_from(total).unwrap())
        .filter(|i| service.serves(*i))
        .count();
    assert!(
        total >= 20,
        "SQS's model has over twenty operations: {total}"
    );
    assert!(built > 0 && built <= total);
    assert_eq!(
        (&sqs["built"], &sqs["total"]),
        (&built.into(), &total.into())
    );
    let apis = sqs["apis"].as_array().unwrap();
    assert_eq!(apis.len(), 1);
    assert_eq!(keys(&apis[0]), ts_keys("ApiCoverage"));
    assert_eq!(
        apis[0],
        serde_json::json!({"sdk_id": "SQS", "signing_name": "sqs", "built": built, "total": total})
    );
}
