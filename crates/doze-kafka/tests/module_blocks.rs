//! The console's working blocks (protocol minor 2) on a running broker, asked as the app asks them: the tree opens the
//! topics, their partitions and their messages (newest first, paging back), a message opens whole with its headers,
//! a group with its lag, and the forms create a topic and produce a message with a key and headers, while an app that
//! does not know forms still produces with its one box.
#![cfg(all(feature = "module", feature = "console"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod module_support;

use std::collections::BTreeMap;
use std::time::Duration;

use doze_module_proto::msg::blocks::{Children, ChildrenArgs, ValueAnswer, ValueArgs, media};
use doze_module_proto::msg::calls::RunAction;
use doze_module_proto::msg::error::Error;
use doze_module_test::Booted;
use kafka_protocol::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_protocol::messages::{ApiKey, ProduceRequest, ProduceResponse, TopicName};
use module_support::{backend, commit, graceful, harness, kafka_call, name, topic_configs};

const BLOCK: &str = r#"kafka "browse" {
  version = 4
  topic "orders" {
    partitions = 2
  }
}
"#;

async fn children(b: &Booted, parent: &str, filter: &str, cursor: Option<&str>) -> Children {
    let a = ChildrenArgs {
        instance: b.instance.clone(),
        panel: "browse".into(),
        parent: parent.into(),
        filter: filter.into(),
        cursor: cursor.map(String::from),
    };
    b.module.children(&a).await.unwrap()
}

async fn value(b: &Booted, id: &str, offset: u64, max: u32) -> Result<ValueAnswer, Error> {
    let a = ValueArgs {
        instance: b.instance.clone(),
        panel: "browse".into(),
        row: BTreeMap::from([("id".to_string(), id.to_string())]),
        offset,
        max,
    };
    b.module.value(&a).await
}

async fn form(
    b: &Booted,
    action: &str,
    resource: &str,
    kv: &[(&str, &str)],
) -> Result<String, Error> {
    let a = RunAction {
        instance: b.instance.clone(),
        action: action.into(),
        resource: resource.into(),
        input: None,
        values: kv
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        form: true,
    };
    b.module.run_action(&a).await
}

fn ids(c: &Children) -> Vec<&str> {
    c.nodes.iter().map(|n| n.id.as_str()).collect()
}

/// Writes a record as any client would (bytes the form cannot type).
fn produce_raw(addr: &str, topic: &str, partition: i32, value: &[u8]) {
    let record = kafka_wire::batch::OneRecord {
        timestamp_ms: 1,
        key: None,
        value: Some(value),
        headers: &[],
    };
    let mut p = PartitionProduceData::default();
    p.index = partition;
    p.records = Some(kafka_wire::batch::single_record(&record));
    let mut t = TopicProduceData::default();
    t.name = TopicName(name(topic));
    t.partition_data.push(p);
    let mut q = ProduceRequest::default();
    q.acks = -1;
    q.timeout_ms = 10_000;
    q.topic_data.push(t);
    let a: ProduceResponse = kafka_call(addr, ApiKey::Produce, 3, &q);
    assert_eq!(a.responses[0].partition_responses[0].error_code, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn the_tree_opens_topics_partitions_and_messages_newest_first_and_pages_back() {
    let b = harness().boot(BLOCK).await.unwrap();
    for i in 0..60 {
        let v = format!("m{i}");
        form(
            &b,
            "produce",
            "orders",
            &[("value", &v), ("partition", "1")],
        )
        .await
        .unwrap();
    }
    assert_eq!(ids(&children(&b, "", "", None).await), ["topics", "groups"]);
    let topics = children(&b, "topics", "", None).await;
    let orders = topics.nodes.iter().find(|n| n.id == "t/orders").unwrap();
    assert!(!orders.leaf);
    assert_eq!(
        (
            orders.info["partitions"].as_str(),
            orders.info["messages"].as_str()
        ),
        ("2", "60")
    );
    let parts = children(&b, "t/orders", "", None).await;
    assert_eq!(ids(&parts), ["t/orders/0", "t/orders/1"]);
    assert_eq!(parts.nodes[1].info["messages"], "60");

    // The newest 50 first, then the 10 older ones.
    let page = children(&b, "t/orders/1", "", None).await;
    assert_eq!(page.nodes.len(), 50);
    assert_eq!(
        (page.nodes[0].label.as_str(), page.nodes[49].label.as_str()),
        ("offset 59", "offset 10")
    );
    assert!(
        page.nodes
            .iter()
            .all(|n| n.leaf && n.info["bytes"].parse::<usize>().unwrap() >= 2)
    );
    assert_eq!(page.next.as_deref(), Some("10"));
    let older = children(&b, "t/orders/1", "", page.next.as_deref()).await;
    assert_eq!(older.nodes.len(), 10);
    assert_eq!(
        (older.nodes[0].id.as_str(), older.next),
        ("t/orders/1/9", None)
    );
    assert!(children(&b, "t/orders/0", "", None).await.nodes.is_empty());

    // A filter searches every name, in any case.
    assert_eq!(ids(&children(&b, "", "ORD", None).await), ["t/orders"]);
    assert!(
        children(&b, "", "nothing-like-it", None)
            .await
            .nodes
            .is_empty()
    );
    graceful(&b.stop(Duration::from_secs(10)).await);
}

#[tokio::test(flavor = "current_thread")]
async fn a_message_opens_with_its_headers_and_a_group_with_its_lag() {
    let b = harness().boot(BLOCK).await.unwrap();
    let json = r#"{"order":7,"total":42.5}"#;
    let said = form(
        &b,
        "produce",
        "orders",
        &[
            ("value", json),
            ("key", "order-7"),
            ("headers", "trace=abc\nsource=api"),
        ],
    )
    .await
    .unwrap();
    // The key chose the partition, as a Java client's would.
    let p = kafka_console::partitioner::for_key(Some(b"order-7"), 2);
    assert_eq!(said, format!("produced to orders, partition {p} offset 0"));
    let leaf = children(&b, &format!("t/orders/{p}"), "", None)
        .await
        .nodes
        .remove(0);
    assert_eq!(
        (leaf.label.as_str(), leaf.info["key"].as_str()),
        ("offset 0", "order-7")
    );
    let v = value(&b, &leaf.id, 0, 0).await.unwrap();
    assert_eq!(
        (v.media.as_str(), v.body.as_slice(), v.more),
        (media::JSON, json.as_bytes(), false)
    );
    let headers = [
        ("trace".to_string(), "abc".to_string()),
        ("source".into(), "api".into()),
    ];
    assert_eq!(v.pairs, headers);
    // A part at a time: the headers come with the first only.
    let first = value(&b, &leaf.id, 0, 5).await.unwrap();
    assert_eq!(
        (first.body.len(), first.size, first.more, first.pairs.len()),
        (5, json.len() as u64, true, 2)
    );
    let rest = value(&b, &leaf.id, 5, 0).await.unwrap();
    assert_eq!(
        (rest.body.as_slice(), rest.more),
        (&json.as_bytes()[5..], false)
    );
    assert!(rest.pairs.is_empty());
    bytes_and_groups(&b, 1 - p).await;
    graceful(&b.stop(Duration::from_secs(10)).await);
}

/// Bytes that are not text are bytes; an offset not held is not found; a group opens as its offsets and lag.
async fn bytes_and_groups(b: &Booted, other: i32) {
    let addr = backend(b);
    produce_raw(&addr, "orders", other, &[0xff, 0x00, 0x89]);
    let v = value(b, &format!("t/orders/{other}/0"), 0, 0)
        .await
        .unwrap();
    assert_eq!(
        (v.media.as_str(), v.body.as_slice()),
        (media::BYTES, &[0xff, 0x00, 0x89][..])
    );
    assert!(
        value(b, &format!("t/orders/{other}/5"), 0, 0)
            .await
            .is_err()
    );
    assert!(
        value(b, "t/orders", 0, 0).await.is_err(),
        "a folder has no value"
    );

    // A group that committed offset 0 of the partition holding one message lags by one.
    commit(&addr, "billing", "orders", 0);
    let groups = children(b, "groups", "", None).await;
    assert_eq!(ids(&groups), ["g/billing"]);
    assert!(groups.nodes[0].leaf);
    let v = value(b, "g/billing", 0, 0).await.unwrap();
    assert_eq!(v.media, media::JSON);
    let doc: serde_json::Value = serde_json::from_slice(&v.body).unwrap();
    assert_eq!(doc["group"], "billing");
    assert_eq!(doc["partitions"][0]["partition"], 0);
    assert_eq!(doc["partitions"][0]["end"], 1);
    assert_eq!(
        doc["lag"], 1,
        "partition 0 holds one message whichever way the key went"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_forms_create_a_topic_and_produce_and_an_old_app_still_produces() {
    let b = harness().boot(BLOCK).await.unwrap();
    let addr = backend(&b);
    let said = form(
        &b,
        "create-topic",
        "",
        &[
            ("name", "payments.settled"),
            ("partitions", "3"),
            ("cleanup", "compact"),
            ("retention_hours", "24"),
        ],
    )
    .await
    .unwrap();
    assert_eq!(said, "created topic payments.settled with 3 partitions");
    let parts = children(&b, "t/payments.settled", "", None).await;
    assert_eq!(parts.nodes.len(), 3);
    let configs = topic_configs(&addr, "payments.settled");
    assert_eq!(configs["cleanup.policy"], "compact");
    assert_eq!(configs["retention.ms"], "86400000");
    for (name, why) in [
        ("orders", "already a topic named orders"),
        ("has space", ""),
    ] {
        let e = form(&b, "create-topic", "", &[("name", name)])
            .await
            .unwrap_err();
        assert_eq!(e.field.as_deref(), Some("name"), "{name}: {e}");
        assert!(e.message.contains(why), "{e}");
    }
    let e = form(
        &b,
        "create-topic",
        "",
        &[("name", "x"), ("replication", "3")],
    )
    .await
    .unwrap_err();
    assert_eq!(e.field.as_deref(), Some("replication"));
    produce_forms(&b).await;
    graceful(&b.stop(Duration::from_secs(10)).await);
}

/// Produce with a form and with one box, as an app that does not know forms sends it.
async fn produce_forms(b: &Booted) {
    // Produce with headers to the partition asked, and refuse one the topic does not have.
    let said = form(
        b,
        "produce",
        "orders",
        &[("value", "hi"), ("headers", "a=1"), ("partition", "1")],
    )
    .await
    .unwrap();
    assert_eq!(said, "produced to orders, partition 1 offset 0");
    assert_eq!(
        value(b, "t/orders/1/0", 0, 0).await.unwrap().pairs,
        [("a".to_string(), "1".to_string())]
    );
    let e = form(
        b,
        "produce",
        "orders",
        &[("value", "hi"), ("partition", "2")],
    )
    .await
    .unwrap_err();
    assert_eq!(e.field.as_deref(), Some("partition"));

    // An app that does not know forms: one box, no values, partition 0, as before.
    let old = RunAction {
        instance: b.instance.clone(),
        action: "produce".into(),
        resource: "orders".into(),
        input: Some("from an old app".into()),
        values: BTreeMap::new(),
        form: false,
    };
    assert_eq!(
        b.module.run_action(&old).await.unwrap(),
        "produced to orders, partition 0 offset 0"
    );
    let v = value(b, "t/orders/0/0", 0, 0).await.unwrap();
    assert_eq!(
        (v.media.as_str(), v.body.as_slice()),
        (media::TEXT, &b"from an old app"[..])
    );
    // Creating a topic needs the form: an old app is told to update.
    let mut old_create = old.clone();
    old_create.action = "create-topic".into();
    let e = b.module.run_action(&old_create).await.unwrap_err();
    assert!(
        e.message.contains(doze_module::forms::NEEDS_NEWER_APP),
        "{e}"
    );
}
