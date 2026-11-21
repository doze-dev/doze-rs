//! Resources pages that once read a service's answer wrongly, each against real services on a real bus.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use serde_json::json;
use world::{encode, field, sub, world};

#[tokio::test]
async fn two_subscriptions_not_yet_confirmed_are_two_rows_with_their_own_pages() {
    let w = world("sns", Mode::Soft, &[]);
    let topic = "arn:aws:sns:us-east-1:000000000000:orders";
    w.round_trip(
        "/api/v1/resources/sns/topics",
        &json!({"Name": "orders"}),
        "orders",
    )
    .await;
    for who in ["ann@example.com", "bob@example.com"] {
        let (status, _) = w
            .send(
                "POST",
                "/api/v1/resources/sns/subscriptions",
                &json!({"TopicArn": topic, "Protocol": "email", "Endpoint": who}),
            )
            .await;
        assert_eq!(status, 201);
    }
    // The list: each is its own row, named by what tells it apart, not by the ARN SNS lists for both.
    let (_, list) = w.get("/api/v1/resources/sns/subscriptions").await;
    let rows = list["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let names: Vec<&str> = rows.iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_ne!(names[0], names[1]);
    assert!(names.iter().all(|n| !n.contains("PendingConfirmation:")));
    assert!(
        rows.iter()
            .any(|r| r["label"] == "ann@example.com (pending)")
    );
    // Each opens, and says what it is.
    for who in ["ann@example.com", "bob@example.com"] {
        let name = format!("pending|{topic}|email|{who}");
        let (status, detail) = w
            .get(&format!(
                "/api/v1/resources/sns/subscriptions/{}",
                encode(&name)
            ))
            .await;
        assert_eq!(status, 200, "{detail}");
        assert_eq!(field(&detail, "Endpoint")["value"], who);
        assert_eq!(field(&detail, "Protocol")["value"], "email");
        assert_eq!(field(&detail, "Pending confirmation")["value"], "Yes");
        assert_eq!(
            field(&detail, "Topic")["link"],
            json!({"service": "sns", "kind": "topics", "name": "orders"})
        );
    }
    // The topic's own list links each row to its page.
    let (_, topic_page) = w.get("/api/v1/resources/sns/topics/orders").await;
    let links: Vec<&str> = topic_page["subs"][0]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["link"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(links.len(), 2);
    assert!(links.iter().all(|l| l.starts_with("pending|")));
    assert_ne!(links[0], links[1]);
    // One that is gone says so.
    let (status, gone) = w
        .get(&format!(
            "/api/v1/resources/sns/subscriptions/{}",
            encode(&format!("pending|{topic}|email|eve@example.com"))
        ))
        .await;
    assert_eq!(status, 404, "{gone}");
}

/// Model `DeleteSecret`: a secret marked for deletion cannot be read ("you cannot retrieve the secret value") until
/// `RestoreSecret` removes the stamp; `DescribeSecret.DeletedDate` is the stamp, so the page says "marked", not "scheduled for".
#[tokio::test]
async fn a_secret_marked_for_deletion_offers_neither_reveal_nor_delete_but_a_way_back() {
    let w = world("secrets", Mode::Soft, &[]);
    let base = "/api/v1/resources/secretsmanager/secrets";
    let name = w
        .round_trip(
            base,
            &json!({"Name": "larder/stripe", "SecretString": "sk_live_1"}),
            "larder/stripe",
        )
        .await;
    let one = format!("{base}/{}", encode(&name));
    let (_, before) = w.get(&one).await;
    assert_eq!(before["reveal"], "Reveal value");
    assert_eq!(before["delete"]["verb"], "Schedule deletion");
    assert_eq!(field(&before, "Value")["hidden"], true);
    let (status, _) = w.send("DELETE", &one, &json!({})).await;
    assert_eq!(status, 200);
    // Still listed (it is there), with the day it was marked.
    let (_, list) = w.get(base).await;
    let row = &list["rows"][0];
    assert_eq!(row["name"], "larder/stripe");
    assert!(row["cells"][4].is_string(), "{row}");
    let (status, marked) = w.get(&one).await;
    assert_eq!(status, 200, "{marked}");
    assert!(field(&marked, "Marked for deletion")["value"].is_string());
    assert!(marked["reveal"].is_null() && marked["delete"].is_null());
    assert_eq!(field(&marked, "Value")["hidden"], false);
    assert!(
        field(&marked, "Value")["value"]
            .as_str()
            .unwrap()
            .contains("Not readable")
    );
    assert_eq!(marked["deep"][0]["operations"], json!(["RestoreSecret"]));
    // Cancel the deletion.
    let (status, _) = w
        .send("POST", &format!("{one}/op/RestoreSecret"), &json!({}))
        .await;
    assert_eq!(status, 200);
    let (_, back) = w.get(&one).await;
    assert_eq!(back["reveal"], "Reveal value");
    assert!(field(&back, "Marked for deletion")["value"].is_null());
}

/// Model `DescribeAlarms.AlarmTypes`: "If you omit this parameter, only metric alarms are returned, even if composite alarms
/// exist": the console names both, and reads the two lists the answer has.
#[tokio::test]
async fn composite_alarms_are_listed_beside_metric_alarms_and_open() {
    let w = world("alarms", Mode::Soft, &[]);
    w.app(
        "monitoring",
        "PutMetricAlarm",
        &json!({"AlarmName": "cpu-high", "MetricName": "CPU", "Namespace": "app", "Statistic": "Average",
            "Period": 60, "EvaluationPeriods": 1, "Threshold": 80.0, "ComparisonOperator": "GreaterThanThreshold"}),
    )
    .await;
    w.app(
        "monitoring",
        "PutCompositeAlarm",
        &json!({"AlarmName": "either", "AlarmRule": "ALARM(cpu-high)"}),
    )
    .await;
    let base = "/api/v1/resources/cloudwatch/alarms";
    let (_, list) = w.get(base).await;
    assert_eq!(list["columns"][0]["title"], "Type");
    let rows: Vec<(String, String)> = list["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["name"].as_str().unwrap().to_owned(),
                r["cells"][0].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("cpu-high".to_owned(), "Metric".to_owned()),
            ("either".to_owned(), "Composite".to_owned())
        ]
    );
    let (status, detail) = w.get(&format!("{base}/either")).await;
    assert_eq!(status, 200, "{detail}");
    assert_eq!(field(&detail, "Rule")["value"], "ALARM(cpu-high)");
    assert!(
        field(&detail, "ARN")["value"]
            .as_str()
            .unwrap()
            .ends_with(":alarm:either")
    );
    // A metric alarm still opens as it did.
    let (_, metric) = w.get(&format!("{base}/cpu-high")).await;
    assert_eq!(field(&metric, "Metric")["value"], "CPU");
    assert!(field(&metric, "Rule")["value"].is_null());
}

#[tokio::test]
async fn an_event_pattern_a_target_transformer_and_an_open_ended_archive_read_as_they_are() {
    let w = world("events", Mode::Soft, &[]);
    w.app(
        "events",
        "PutRule",
        &json!({"Name": "on-order", "EventPattern": r#"{"source":["larder.orders"],"detail-type":["OrderPlaced"]}"#}),
    )
    .await;
    w.app("sqs", "CreateQueue", &json!({"QueueName": "inbox"}))
        .await;
    w.app(
        "events",
        "PutTargets",
        &json!({"Rule": "on-order", "Targets": [
            {"Id": "t1", "Arn": "arn:aws:sqs:us-east-1:000000000000:inbox",
             "InputTransformer": {"InputPathsMap": {"id": "$.detail.id"}, "InputTemplate": "{\"order\": <id>}"}},
            {"Id": "t2", "Arn": "arn:aws:sqs:us-east-1:000000000000:inbox", "Input": "{\"fixed\":true}"}
        ]}),
    )
    .await;
    let (status, rule) = w.get("/api/v1/resources/eventbridge/rules/on-order").await;
    assert_eq!(status, 200, "{rule}");
    // The pattern: one member to a line, each short list on its line.
    assert_eq!(
        field(&rule, "Event pattern")["value"],
        "{\n  \"source\": [\"larder.orders\"],\n  \"detail-type\": [\"OrderPlaced\"]\n}"
    );
    let targets = &sub(&rule, "Targets")["rows"];
    let input_at = sub(&rule, "Targets")["columns"]
        .as_array()
        .unwrap()
        .iter()
        .position(|c| c["title"] == "Input")
        .unwrap();
    let inputs: Vec<&str> = targets
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["cells"][input_at].as_str().unwrap())
        .collect();
    assert_eq!(inputs, ["{\"order\": <id>}", "{\"fixed\":true}"]);
    // An archive that keeps its events without end says so, in the list and on its page.
    w.app(
        "events",
        "CreateArchive",
        &json!({"ArchiveName": "all", "EventSourceArn": "arn:aws:events:us-east-1:000000000000:event-bus/default"}),
    )
    .await;
    let (_, list) = w.get("/api/v1/resources/eventbridge/archives").await;
    let at = list["columns"]
        .as_array()
        .unwrap()
        .iter()
        .position(|c| c["title"] == "Retention (days)")
        .unwrap();
    assert_eq!(list["rows"][0]["cells"][at], "Indefinite");
    let (_, archive) = w.get("/api/v1/resources/eventbridge/archives/all").await;
    assert_eq!(field(&archive, "Retention (days)")["value"], "Indefinite");
}

/// A REST API made from the console asks for a regional endpoint (the form's default); API Gateway's own default is EDGE.
#[tokio::test]
async fn a_rest_api_made_from_the_form_is_regional_and_titled_by_its_name() {
    let w = world("rest", Mode::Soft, &[]);
    let base = "/api/v1/resources/apigateway/restapis";
    let form = w.get(base).await.1;
    let input = form["create"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["key"] == "endpointType")
        .unwrap()
        .clone();
    assert_eq!(input["value"], "REGIONAL");
    let id = w
        .round_trip(
            base,
            &json!({"name": "orders-api", "endpointType": input["value"]}),
            "orders-api",
        )
        .await;
    let (_, detail) = w.get(&format!("{base}/{id}")).await;
    assert_eq!(field(&detail, "Endpoint")["value"], "REGIONAL");
    assert_eq!(field(&detail, "Name")["value"], "orders-api");
    // The form is refused without it, rather than making an API with no endpoint type at all.
    let (status, refused) = w.send("POST", base, &json!({"name": "edge-api"})).await;
    assert_eq!(status, 400, "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("Endpoint type is required")
    );
}
