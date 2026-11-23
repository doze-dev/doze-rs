//! A queue's page of its own: the list's columns and its name-prefix search, the subscriptions that deliver to the queue, and the
//! operations the page makes on other services (the rules that feed it, a topic's subscription, a Lambda trigger), each bound to
//! this queue. Against real services on a real bus.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

#[path = "resources/code.rs"]
mod code;
mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use serde_json::{Value, json};
use world::{field, firsts, sub, world};

const BASE: &str = "/api/v1/resources";
const ACCOUNT: &str = "000000000000";

fn arn(queue: &str) -> String {
    format!("arn:aws:sqs:us-east-1:{ACCOUNT}:{queue}")
}

#[tokio::test]
async fn the_list_reads_encryption_and_deduplication_and_searches_by_the_names_prefix() {
    let w = world("sqs-list", Mode::Enforce, &[]);
    for (name, attrs) in [
        ("pick-lists", json!({})),
        ("pick-lists-dlq", json!({})),
        ("analytics-queue", json!({"KmsMasterKeyId": "alias/app"})),
        (
            "orders-events.fifo",
            json!({"FifoQueue": "true", "ContentBasedDeduplication": "true"}),
        ),
    ] {
        w.app(
            "sqs",
            "CreateQueue",
            &json!({"QueueName": name, "Attributes": attrs}),
        )
        .await;
    }
    let (status, list) = w.get(&format!("{BASE}/sqs/queues")).await;
    assert_eq!(status, 200, "{list}");
    let titles: Vec<&str> = list["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        [
            "Type",
            "Messages available",
            "In flight",
            "Dead-letter queue",
            "Created",
            "SQS-managed encryption",
            "KMS key",
            "Content-based deduplication"
        ]
    );
    let cells = |name: &str| -> Vec<Value> {
        list["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name)
            .unwrap()["cells"]
            .as_array()
            .unwrap()
            .clone()
    };
    // The page reads the last three as Encryption and Content-based deduplication.
    assert_eq!(cells("pick-lists")[5], "Enabled");
    assert_eq!(cells("pick-lists")[7], "");
    assert_eq!(cells("analytics-queue")[6], "alias/app");
    assert_eq!(cells("orders-events.fifo")[7], "Enabled");

    // The search is ListQueues' QueueNamePrefix: a prefix, and case-sensitive.
    let (_, found) = w.get(&format!("{BASE}/sqs/queues?p.prefix=pick")).await;
    assert_eq!(firsts_names(&found), ["pick-lists", "pick-lists-dlq"]);
    assert_eq!(found["params"][0]["key"], "prefix");
    assert_eq!(found["params"][0]["value"], "pick");
    let (_, none) = w.get(&format!("{BASE}/sqs/queues?p.prefix=Pick")).await;
    assert_eq!(firsts_names(&none), Vec::<String>::new());
    assert_eq!(none["sentence"], Value::Null);
    assert_eq!(list["sentence"], "4 queues hold 0 messages waiting.");
}

fn firsts_names(list: &Value) -> Vec<String> {
    list["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn a_queue_lists_the_topics_that_deliver_to_it_and_no_others() {
    let w = world("sqs-subs", Mode::Enforce, &[]);
    for q in ["invoice-queue", "pick-lists"] {
        w.app("sqs", "CreateQueue", &json!({"QueueName": q})).await;
    }
    let topic = w
        .app("sns", "CreateTopic", &json!({"Name": "orders-created"}))
        .await["TopicArn"]
        .as_str()
        .unwrap()
        .to_owned();
    w.app(
        "sns",
        "Subscribe",
        &json!({"TopicArn": topic, "Protocol": "sqs", "Endpoint": arn("invoice-queue")}),
    )
    .await;
    let (_, mine) = w.get(&format!("{BASE}/sqs/queues/invoice-queue")).await;
    let subs = sub(&mine, "SNS subscriptions");
    assert_eq!(subs["operation"], "ListSubscriptions");
    assert_eq!(firsts(subs), ["orders-created"]);
    assert_eq!(subs["rows"][0]["link"]["kind"], "topics");
    // A queue nothing delivers to has none, though the account has a subscription.
    let (_, other) = w.get(&format!("{BASE}/sqs/queues/pick-lists")).await;
    assert_eq!(
        firsts(sub(&other, "SNS subscriptions")),
        Vec::<String>::new()
    );
    // The page also reads what a FIFO queue and a KMS-encrypted queue say about themselves.
    w.app(
        "sqs",
        "CreateQueue",
        &json!({"QueueName": "stock.fifo", "Attributes": {"FifoQueue": "true", "KmsMasterKeyId": "alias/app"}}),
    )
    .await;
    let (_, fifo) = w.get(&format!("{BASE}/sqs/queues/stock.fifo")).await;
    assert_eq!(field(&fifo, "FIFO throughput limit")["value"], "perQueue");
    assert_eq!(field(&fifo, "Deduplication scope")["value"], "queue");
    assert_eq!(field(&fifo, "KMS key")["value"], "alias/app");
}

#[tokio::test]
async fn the_pages_cross_service_operations_act_on_this_queue_and_no_other() {
    let w = world("sqs-bound", Mode::Enforce, &[]);
    for q in ["pick-lists", "other"] {
        w.app("sqs", "CreateQueue", &json!({"QueueName": q})).await;
    }
    w.app("events", "CreateEventBus", &json!({"Name": "larder"}))
        .await;
    w.app(
        "events",
        "PutRule",
        &json!({"Name": "payment-settled", "EventBusName": "larder", "EventPattern": "{\"source\":[\"pay\"]}"}),
    )
    .await;
    w.app(
        "events",
        "PutTargets",
        &json!({"Rule": "payment-settled", "EventBusName": "larder",
            "Targets": [{"Id": "1", "Arn": arn("pick-lists")}]}),
    )
    .await;
    let post = |op: &str, body: Value| {
        let path = format!("{BASE}/sqs/queues/pick-lists/op/{op}");
        let w = &w;
        async move { w.send("POST", &path, &body).await }
    };

    // The rules that feed the queue: the buses there are, then each bus's rules that target this queue's ARN.
    let (status, buses) = post("ListEventBuses", json!({})).await;
    assert_eq!(status, 200, "{buses}");
    let names: Vec<&str> = buses["EventBuses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["Name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["default", "larder"]);
    let (_, rules) = post("ListRuleNamesByTarget", json!({"EventBusName": "larder"})).await;
    assert_eq!(rules["RuleNames"], json!(["payment-settled"]));
    // The target is this queue's whatever the page sends: another queue's rules are not read through this page.
    let (_, rules) = post(
        "ListRuleNamesByTarget",
        json!({"EventBusName": "larder", "TargetArn": arn("other")}),
    )
    .await;
    assert_eq!(rules["RuleNames"], json!(["payment-settled"]));

    // A subscription: the endpoint is this queue, the topic is of this account and Region.
    let topic = w
        .app("sns", "CreateTopic", &json!({"Name": "orders-created"}))
        .await["TopicArn"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, done) = post(
        "Subscribe",
        json!({"TopicArn": topic, "Protocol": "http", "Endpoint": arn("other")}),
    )
    .await;
    assert_eq!(status, 200, "{done}");
    let (_, mine) = w.get(&format!("{BASE}/sqs/queues/pick-lists")).await;
    assert_eq!(firsts(sub(&mine, "SNS subscriptions")), ["orders-created"]);
    let (_, other) = w.get(&format!("{BASE}/sqs/queues/other")).await;
    assert_eq!(
        firsts(sub(&other, "SNS subscriptions")),
        Vec::<String>::new()
    );
    let (status, refused) = post(
        "Subscribe",
        json!({"TopicArn": "arn:aws:sns:eu-west-1:000000000000:alerts"}),
    )
    .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (400, Some("NOT_THIS_RESOURCE"))
    );
    let (status, refused) = post(
        "Subscribe",
        json!({"TopicArn": "arn:aws:sns:us-east-1:1:x"}),
    )
    .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (400, Some("NOT_THIS_RESOURCE"))
    );

    // Every call is the console's, on the service it belongs to.
    let live = w.live();
    for want in [
        "console events ListEventBuses",
        "console events ListRuleNamesByTarget",
        "console sns Subscribe",
    ] {
        assert!(live.iter().any(|l| l == want), "{want} in {live:?}");
    }
    // An operation no page of the queue lists is not made, on any service.
    let (status, refused) = post("DeleteEventBus", json!({"Name": "larder"})).await;
    assert_eq!((status, refused["code"].as_str()), (404, Some("NOT_FOUND")));
}

#[tokio::test]
async fn a_lambda_trigger_polls_this_queue_whatever_source_the_page_names() {
    let w = world("sqs-trigger", Mode::Enforce, &[]);
    if !code::function(&w, "send-email").await {
        return;
    }
    for q in ["pick-lists", "other"] {
        w.app("sqs", "CreateQueue", &json!({"QueueName": q})).await;
    }
    let (status, made) = w
        .send(
            "POST",
            &format!("{BASE}/sqs/queues/pick-lists/op/CreateEventSourceMapping"),
            &json!({"FunctionName": "send-email", "EventSourceArn": arn("other")}),
        )
        .await;
    assert_eq!(status, 200, "{made}");
    assert_eq!(made["EventSourceArn"], arn("pick-lists"));
    let (_, d) = w.get(&format!("{BASE}/sqs/queues/pick-lists")).await;
    assert_eq!(firsts(sub(&d, "Lambda triggers")), ["send-email"]);
    let (_, d) = w.get(&format!("{BASE}/sqs/queues/other")).await;
    assert_eq!(firsts(sub(&d, "Lambda triggers")), Vec::<String>::new());
}
