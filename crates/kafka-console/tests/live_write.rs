//! The topic writes over a real broker in this process: a page is a form, a POST makes a protocol request as the console's
//! principal, and what the broker did is looked at through the protocol (Metadata, DescribeConfigs, ListOffsets, Fetch).
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_write;

use axum::http::StatusCode;
use support::get;
use support_write::{page, post_form, world};

fn create_fields<'a>(
    name: &'a str,
    partitions: &'a str,
    cleanup: &'a str,
    retention: &'a str,
    mode: &'a str,
) -> Vec<(&'a str, &'a str)> {
    vec![
        ("name", name),
        ("partitions", partitions),
        ("template", "event"),
        ("cleanup.policy", cleanup),
        ("retention.ms", retention),
        ("mode", mode),
    ]
}

#[tokio::test]
async fn a_topic_made_through_the_pages_exists_for_the_protocol_with_its_partitions_and_configs() {
    let w = world("create", |_, _| {}).await;
    let app = w.router();
    // The three steps carry what was typed.
    let (status, step1) = page(&app, get("/topics/new")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        step1.contains("<h1>Create topic</h1>") && step1.contains("of 1 to 1,000"),
        "{step1:.300}"
    );
    let (_, step2) = page(
        &app,
        get("/topics/new?step=config&name=inventory.levels&partitions=2&template=compact"),
    )
    .await;
    assert!(
        step2.contains("cleanup.policy") && step2.contains("<b>Kafka says</b>"),
        "{step2:.300}"
    );
    assert!(
        step2.contains("value=\"compact\" checked"),
        "the compacted starting point sets the policy"
    );
    let (_, step3) = page(&app, get("/topics/new?step=review&name=inventory.levels&partitions=2&template=compact&cleanup.policy=compact&retention.ms=3600000")).await;
    assert!(
        step3.contains("What you will get") && step3.contains("kafka-topics.sh --bootstrap-server"),
        "{step3:.300}"
    );
    // "Check only" runs the checks and makes nothing.
    let fields = create_fields("inventory.levels", "2", "compact", "3600000", "check");
    let (status, checked) = page(&app, post_form("/topics/new", &fields)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(checked.contains("<b>Checked.</b>"), "{checked:.300}");
    assert!(w.topics().await.is_empty(), "a dry run made a topic");
    // Create it.
    let fields = create_fields("inventory.levels", "2", "compact", "3600000", "create");
    let (status, made) = page(&app, post_form("/topics/new", &fields)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        made.contains("Created inventory.levels.")
            && made.contains("2 partitions, cleanup.policy compact, retention 1 h"),
        "{made:.600}"
    );
    assert!(
        made.contains("Empty: offsets 0 to 0 on all 2 partitions."),
        "the page checked it with ListOffsets"
    );
    assert_eq!(w.topics().await, [("inventory.levels".to_owned(), 2)]);
    assert_eq!(
        w.config("inventory.levels", "cleanup.policy")
            .await
            .as_deref(),
        Some("compact")
    );
    assert_eq!(
        w.config("inventory.levels", "retention.ms")
            .await
            .as_deref(),
        Some("3600000")
    );
    w.stop().await;
}

#[tokio::test]
async fn every_refusal_of_a_create_is_kafkas_code_and_the_brokers_own_sentence() {
    let w = world("create-refused", |_, _| {}).await;
    w.create_topic("orders", 1, &[]).await;
    w.create_topic("orders.dlq", 1, &[]).await;
    let app = w.router();
    for (fields, code, words) in [
        (
            create_fields("orders", "1", "delete", "1000", "create"),
            "TOPIC_ALREADY_EXISTS (36)",
            "Topic 'orders' already exists.",
        ),
        (
            create_fields("orders_dlq", "1", "delete", "1000", "create"),
            "INVALID_TOPIC_EXCEPTION (17)",
            "Topic 'orders_dlq' collides with existing topic: orders.dlq",
        ),
        (
            create_fields("bad name", "1", "delete", "1000", "create"),
            "INVALID_TOPIC_EXCEPTION (17)",
            "Topic name is invalid: 'bad name' contains one or more characters other than ASCII alphanumerics",
        ),
        (
            create_fields("..", "1", "delete", "1000", "create"),
            "INVALID_TOPIC_EXCEPTION (17)",
            "Topic name is invalid: '..' is not allowed",
        ),
        (
            create_fields("fresh", "1001", "delete", "1000", "create"),
            "INVALID_PARTITIONS (37)",
            "1001 partitions is above this broker's limit of 1000 (--max-partitions)",
        ),
        (
            create_fields("fresh", "0", "delete", "1000", "create"),
            "INVALID_PARTITIONS (37)",
            "",
        ),
        (
            create_fields("fresh", "1", "delete", "7d", "create"),
            "INVALID_CONFIG (40)",
            "Invalid value 7d for configuration retention.ms",
        ),
        (
            create_fields("fresh", "1", "shred", "1000", "create"),
            "INVALID_CONFIG (40)",
            "Invalid value shred for configuration cleanup.policy",
        ),
    ] {
        let (status, doc) = page(&app, post_form("/topics/new", &fields)).await;
        assert_eq!(status, StatusCode::OK, "{fields:?}");
        assert!(
            doc.contains(&format!("Refused · {code}")),
            "{fields:?} -> {doc:.800}"
        );
        assert!(doc.contains("The topic was not created."), "{fields:?}");
        let escaped = words.replace('&', "&amp;").replace('<', "&lt;");
        assert!(
            doc.contains(&escaped),
            "{fields:?}: {words:?} is not in {doc:.1500}"
        );
    }
    assert_eq!(w.topics().await.len(), 2, "no refusal made a topic");
    // A name that is taken offers the topic that is there; a name Kafka forbids does not.
    let (_, taken) = page(
        &app,
        post_form(
            "/topics/new",
            &create_fields("orders", "1", "delete", "1000", "create"),
        ),
    )
    .await;
    assert!(
        taken.contains("Open the existing topic") && taken.contains("href=\"/topics/orders\""),
        "{taken:.400}"
    );
    let (_, bad) = page(
        &app,
        post_form(
            "/topics/new",
            &create_fields("a b", "1", "delete", "1000", "create"),
        ),
    )
    .await;
    assert!(!bad.contains("Open the existing topic"));
    // A count that is not a number cannot be sent: the first step again, with the error, as 422.
    let (status, nan) = page(
        &app,
        post_form(
            "/topics/new",
            &create_fields("fresh", "three", "delete", "1000", "create"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        nan.contains("A whole number is needed, such as 3."),
        "{nan:.300}"
    );
    w.stop().await;
}

#[tokio::test]
async fn a_keyed_and_a_keyless_record_are_written_and_fetched_back_through_the_protocol() {
    let w = world("produce", |_, _| {}).await;
    w.create_topic("orders", 3, &[]).await;
    let app = w.router();
    let keyed = [
        ("partition", "2"),
        ("key", "ord-88420"),
        ("value", "{\"order_id\":\"ord-88420\"}"),
        ("hn0", "content-type"),
        ("hv0", "application/json"),
        ("acks", "-1"),
    ];
    let (status, sent) = page(&app, post_form("/topics/orders/produce", &keyed)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        sent.contains("Sent to orders-2 at offset 0."),
        "{sent:.500}"
    );
    assert!(
        sent.contains("acknowledged after the flush (acks -1)") && sent.contains("Committed"),
        "{sent:.800}"
    );
    assert_eq!(w.offset("orders", 2, -1).await, 1);
    let bytes = w.fetch("orders", 2, 0).await;
    for want in [
        &b"ord-88420"[..],
        b"{\"order_id\":\"ord-88420\"}",
        b"content-type",
        b"application/json",
    ] {
        assert!(
            bytes.windows(want.len()).any(|x| x == want),
            "{:?} is not in the fetched batch",
            String::from_utf8_lossy(want)
        );
    }
    // Left to the key, the record goes where Kafka's default partitioner puts it (murmur2("21") is 1173551340 made positive).
    let auto = [
        ("partition", ""),
        ("key", "21"),
        ("value", "x"),
        ("acks", "1"),
    ];
    let (_, doc) = page(&app, post_form("/topics/orders/produce", &auto)).await;
    assert!(doc.contains("Sent to orders-0 at offset 0."), "{doc:.400}");
    // A record with no key goes to a topic that keeps by delete, and gets the next offset.
    let keyless = [
        ("partition", "2"),
        ("key", ""),
        ("value", "no key here"),
        ("acks", "-1"),
    ];
    let (_, doc) = page(&app, post_form("/topics/orders/produce", &keyless)).await;
    assert!(doc.contains("Sent to orders-2 at offset 1."), "{doc:.400}");
    assert_eq!(w.offset("orders", 2, -1).await, 2);
    // An acks the broker does not take, and a partition the topic does not have, are told before anything is sent.
    let (status, bad) = page(
        &app,
        post_form(
            "/topics/orders/produce",
            &[("partition", "9"), ("acks", "7"), ("value", "x")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        bad.contains("acks is -1 (all), 1 or 0.")
            && bad.contains("This topic has partitions 0 to 2."),
        "{bad:.300}"
    );
    assert_eq!(w.offset("orders", 2, -1).await, 2, "nothing was written");
    w.stop().await;
}

#[tokio::test]
async fn kafkas_own_words_for_a_keyless_record_on_a_compacted_topic_and_for_a_record_that_is_too_large()
 {
    let w = world("produce-refused", |_, _| {}).await;
    w.create_topic("inventory.levels", 1, &[("cleanup.policy", "compact")])
        .await;
    w.create_topic("tiny", 1, &[("max.message.bytes", "1000")])
        .await;
    let app = w.router();
    let (status, doc) = page(
        &app,
        post_form(
            "/topics/inventory.levels/produce",
            &[
                ("key", ""),
                ("value", "{\"sku\":\"FV-TOM-V\"}"),
                ("acks", "-1"),
            ],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        doc.contains("<h1 class=\"mono\">inventory.levels</h1>")
            && doc.contains("A compacted topic keeps the latest record for every key."),
        "{doc:.500}"
    );
    assert!(doc.contains("Refused · INVALID_RECORD (87)"), "{doc:.1200}");
    assert!(doc.contains("Compacted topic cannot accept message without key in topic partition inventory.levels-0"), "{doc:.2000}");
    assert!(
        doc.contains("give the record a key")
            && doc.contains("A compacted topic needs a key on every record.")
    );
    assert_eq!(
        w.offset("inventory.levels", 0, -1).await,
        0,
        "no offset was used"
    );
    // With a key it is accepted, and so is a key with no value (a deletion).
    let (_, ok) = page(
        &app,
        post_form(
            "/topics/inventory.levels/produce",
            &[("key", "FV-TOM-V"), ("value", "{}"), ("acks", "-1")],
        ),
    )
    .await;
    assert!(
        ok.contains("Sent to inventory.levels-0 at offset 0."),
        "{ok:.300}"
    );
    let (_, tomb) = page(
        &app,
        post_form(
            "/topics/inventory.levels/produce",
            &[("key", "FV-TOM-V"), ("value", ""), ("acks", "-1")],
        ),
    )
    .await;
    assert!(
        tomb.contains("Sent to inventory.levels-0 at offset 1."),
        "{tomb:.300}"
    );
    // Too large for this topic.
    let big = "x".repeat(5000);
    let (_, large) = page(
        &app,
        post_form(
            "/topics/tiny/produce",
            &[("key", "k"), ("value", &big), ("acks", "-1")],
        ),
    )
    .await;
    assert!(
        large.contains("Refused · MESSAGE_TOO_LARGE (10)"),
        "{large:.1500}"
    );
    assert!(
        large.contains("a record under 1,000 bytes (max.message.bytes)"),
        "the limit was read from the topic: {large:.2500}"
    );
    assert_eq!(w.offset("tiny", 0, -1).await, 0);
    // A topic that is not there is not a topic.
    let (status, none) = page(&app, post_form("/topics/ghost/produce", &[("value", "x")])).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(none.contains("There is no topic called"));
    w.stop().await;
}
