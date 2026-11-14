//! The module face of doze-kafka (docs/port/phase11.md), driven as doze drives it: the binary started as
//! `kafka-plugin` by the host, over module protocol 2, and the core's boot sequence from `doze-module-test`.
//!
//! - against the Go kafka module's recorded answers (`tools/module-oracle/kafka`): every case differs only as listed;
//! - the published example and the Larder example's block boot on the broker and converge;
//! - better than Go, each on the running broker: converge raises partitions and sets changed configs, and never
//!   removes; the console's numbers, produce and tail; a sleep and a wake keep the data.
//!
//! The default build (module and console) is the one doze ships as its kafka module, and the one these run on.
#![cfg(all(feature = "module", feature = "console"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod module_support;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use doze_module_proto::msg::calls::RunAction;
use doze_module_proto::msg::console::{Row, TailArgs};
use doze_module_proto::msg::instance::Address;
use doze_module_test::hcl;
use module_support::{
    ask_for, backend, case_hcl, commit, find, graceful, harness, resources, set_topic_configs,
    topic_configs, with,
};

#[tokio::test(flavor = "current_thread")]
async fn the_published_example_boots_on_the_broker_with_its_topics_and_stops_cleanly() {
    let b = harness().boot_example().await.unwrap();
    assert!(b.fresh);
    let rs = resources(&b).await;
    assert_eq!(find(&rs, "topic", "orders").info["partitions"], "3");
    assert_eq!(find(&rs, "topic", "payments").info["partitions"], "1");
    let addr = backend(&b);
    assert_eq!(
        topic_configs(&addr, "payments")
            .get("cleanup.policy")
            .map(String::as_str),
        Some("compact")
    );
    // The broker serves the Kafka major the block names, and its console answers on the instance's name.
    b.module.health(&b.instance).await.unwrap();
    let console = match &b.instance.listeners["console"].backend {
        Address::Tcp(a) => a.clone(),
        other => panic!("{other:?}"),
    };
    let host = b.instance.listeners["console"].hosts[0].clone();
    let mut s = TcpStream::connect(&console).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(
        s,
        "GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut answer = String::new();
    let _ = s.read_to_string(&mut answer);
    assert!(
        answer.starts_with("HTTP/1.1 200"),
        "{}",
        answer.lines().next().unwrap_or_default()
    );
    let instance = b.instance.clone();
    let module = b.module.clone();
    graceful(&b.stop(Duration::from_secs(10)).await);
    assert!(
        module.health(&instance).await.is_err(),
        "a stopped broker is not healthy"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_larder_examples_block_boots_with_its_four_topics_and_no_auto_create() {
    let b = harness().boot(&case_hcl("kafka/119-larder")).await.unwrap();
    let rs = resources(&b).await;
    for t in [
        "users.signed-up",
        "orders.placed",
        "payments.settled",
        "payments.failed",
    ] {
        assert_eq!(find(&rs, "topic", t).info["partitions"], "3", "{t}");
    }
    // auto_create_topics = false reached the broker: a topic no one declared is not made by asking for it.
    let addr = backend(&b);
    assert_eq!(
        ask_for(&addr, "not.declared"),
        3,
        "UNKNOWN_TOPIC_OR_PARTITION"
    );
    let c = b.module.connection(&b.instance).await.unwrap();
    assert_eq!(c.env_var, "KAFKA_BROKERS");
    assert_eq!(c.url, b.instance.listeners["main"].addr);
    graceful(&b.stop(Duration::from_secs(10)).await);
}

const GROWING: &str = r#"kafka "grow" {
  version = 3
  topic "orders" {
    partitions = 2
    config = { "retention.ms" = 3600000 }
  }
}
"#;

#[tokio::test(flavor = "current_thread")]
async fn converge_raises_partitions_and_sets_changed_configs_and_never_removes() {
    let b = harness().boot(GROWING).await.unwrap();
    let addr = backend(&b);
    assert_eq!(
        find(&resources(&b).await, "topic", "orders").info["partitions"],
        "2"
    );
    assert_eq!(topic_configs(&addr, "orders")["retention.ms"], "3600000");
    // Someone sets a config the block does not mention: converge leaves it.
    let mut configs = topic_configs(&addr, "orders");
    configs.insert("segment.ms".into(), "600000".into());
    set_topic_configs(&addr, "orders", &configs);
    // The block changes: more partitions, a changed config and a new one. Go would have created nothing and changed
    // nothing, the topic existing.
    let changed = GROWING.replace("partitions = 2", "partitions = 5").replace(
        "{ \"retention.ms\" = 3600000 }",
        "{ \"retention.ms\" = 7200000, \"cleanup.policy\" = \"compact\" }",
    );
    let mut w = with(&b);
    w.instance.spec = hcl::parse_block(&changed, &b.module.manifest.schema)
        .unwrap()
        .spec;
    b.module.converge(&w).await.unwrap();
    assert_eq!(
        find(&resources(&b).await, "topic", "orders").info["partitions"],
        "5"
    );
    let now = topic_configs(&addr, "orders");
    assert_eq!(now["retention.ms"], "7200000");
    assert_eq!(now["cleanup.policy"], "compact");
    assert_eq!(
        now["segment.ms"], "600000",
        "a config the block does not name is kept"
    );
    // Fewer partitions than the topic has: Kafka cannot remove them, so converge keeps them and succeeds.
    w.instance.spec = hcl::parse_block(
        &GROWING.replace("partitions = 2", "partitions = 1"),
        &b.module.manifest.schema,
    )
    .unwrap()
    .spec;
    b.module.converge(&w).await.unwrap();
    assert_eq!(
        find(&resources(&b).await, "topic", "orders").info["partitions"],
        "5"
    );
    // A block without the topic: nothing is deleted (the module has no prune).
    w.instance.spec = hcl::parse_block(
        "kafka \"grow\" {\n  version = 3\n}\n",
        &b.module.manifest.schema,
    )
    .unwrap()
    .spec;
    b.module.converge(&w).await.unwrap();
    assert!(b.module.objects(&w.instance).await.unwrap().is_empty());
    assert_eq!(
        find(&resources(&b).await, "topic", "orders").info["partitions"],
        "5"
    );
    graceful(&b.stop(Duration::from_secs(10)).await);
}

#[tokio::test(flavor = "current_thread")]
async fn the_app_sees_topics_and_lag_produces_tails_and_a_wake_keeps_the_data() {
    let b = harness().boot_example().await.unwrap();
    let addr = backend(&b);
    let produce = |value: &str| RunAction {
        instance: b.instance.clone(),
        action: "produce".into(),
        resource: "orders".into(),
        input: Some(value.into()),
    };
    assert_eq!(
        b.module.run_action(&produce("first")).await.unwrap(),
        "produced to orders, partition 0 offset 0"
    );
    let mut bad = produce("x");
    bad.action = "purge".into();
    assert!(b.module.run_action(&bad).await.is_err());
    // A consumer group that committed nothing of what was produced since lags by it.
    commit(&addr, "billing", "orders", 0);
    assert_eq!(
        b.module.run_action(&produce("second")).await.unwrap(),
        "produced to orders, partition 0 offset 1"
    );
    let rs = resources(&b).await;
    let orders = find(&rs, "topic", "orders");
    assert_eq!(orders.info["high"], "2");
    assert_eq!(orders.info["parts"], "2,0,0");
    assert_eq!(orders.status, "3 partitions · high-water 2");
    let billing = find(&rs, "group", "billing");
    assert_eq!(billing.info["lag"], "2");
    assert_eq!(billing.info["members"], "0");

    // The tail starts with what the topic holds, then shows what arrives.
    let args = TailArgs {
        panel: "tail".into(),
        selector: "orders".into(),
        instance: b.instance.clone(),
    };
    let mut stream = b.module.tail(&args).unwrap();
    let mut next = async || -> Row {
        tokio::time::timeout(Duration::from_secs(10), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    };
    let first = next().await;
    assert_eq!(
        (
            first.cells["value"].as_str(),
            first.cells["offset"].as_str()
        ),
        ("first", "0")
    );
    assert_eq!(next().await.cells["value"], "second");
    let long = "x".repeat(1000);
    b.module.run_action(&produce(&long)).await.unwrap();
    let row = next().await;
    assert_eq!(row.cells["offset"], "2");
    assert!(row.cells["value"].ends_with('…') && row.cells["value"].len() < 300);
    drop(stream);

    // Doze puts an idle broker to sleep and wakes it for the next client: the messages are still there.
    let (stops, awake) = b.sleep_and_wake(Duration::from_secs(10)).await.unwrap();
    graceful(&stops);
    assert!(!awake.fresh);
    assert_eq!(
        find(&resources(&awake).await, "topic", "orders").info["high"],
        "3"
    );
    graceful(&awake.stop(Duration::from_secs(10)).await);
}

#[tokio::test(flavor = "current_thread")]
async fn a_mistake_is_refused_before_boot_at_its_line() {
    let e = harness()
        .boot("kafka \"bad\" {\n  version = 4\n  topic \"orders\" {\n    config = { \"retention.msec\" = 1 }\n  }\n}\n")
        .await
        .unwrap_err();
    assert!(
        e.contains("Unknown topic config name: retention.msec"),
        "{e}"
    );
    assert!(e.contains("did you mean \"retention.ms\"?"), "{e}");
    let e = harness()
        .boot("kafka \"bad\" {\n  version = \"3.7\"\n}\n")
        .await
        .unwrap_err();
    assert!(e.contains("version = 3"), "{e}");
}
