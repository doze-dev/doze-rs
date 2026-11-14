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

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use bytes::{Buf, Bytes};
use doze_module_proto::msg::calls::RunAction;
use doze_module_proto::msg::console::{Resource, Row, TailArgs};
use doze_module_proto::msg::hello::caps;
use doze_module_proto::msg::instance::Address;
use doze_module_proto::msg::run::WithToolchain;
use doze_module_test::{Booted, Harness, Stops, hcl, oracle};
use doze_os::tree::Stopped;
use kafka_protocol::messages::describe_configs_request::DescribeConfigsResource;
use kafka_protocol::messages::metadata_request::MetadataRequestTopic;
use kafka_protocol::messages::offset_commit_request::{
    OffsetCommitRequestPartition, OffsetCommitRequestTopic,
};
use kafka_protocol::messages::{
    ApiKey, DescribeConfigsRequest, DescribeConfigsResponse, GroupId, MetadataRequest,
    MetadataResponse, OffsetCommitRequest, OffsetCommitResponse, ResponseHeader, TopicName,
};
use kafka_protocol::protocol::{Decodable, Encodable, StrBytes};
use kafka_wire::testing::{framed, request_payload};
use serde_json::{Value as Json, json};

const CORPUS: &str = include_str!("../../../tools/module-oracle/kafka/corpus.jsonl");
const GOLDEN: &str = include_str!("../../../tools/module-oracle/kafka/golden.jsonl");

/// The binary under the name a module release gives it (`bin/kafka-plugin`), which is how doze starts the module: a
/// link to the built `doze-kafka`, made once per test process next to the other build outputs.
fn plugin() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("kafka-module-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = if cfg!(windows) {
            "kafka-plugin.exe"
        } else {
            "kafka-plugin"
        };
        let link = dir.join(exe);
        let _ = std::fs::remove_file(&link);
        let built = env!("CARGO_BIN_EXE_doze-kafka");
        if std::fs::hard_link(built, &link).is_err() {
            std::fs::copy(built, &link).unwrap();
        }
        link
    })
    .clone()
}

fn harness() -> Harness {
    Harness {
        program: plugin(),
        engine: "kafka".into(),
        bin_dir: PathBuf::from("/unused"),
        full: "4".into(),
    }
}

/// Whether the broker in this build serves a schema registry (its flag is the Go edition's).
fn broker_has_registry() -> bool {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
        .arg("--help")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).contains("--schema-registry-addr")
}

// ---------------------------------------------------------------------------------------------------------------------
// The oracle.

fn case_hcl(id: &str) -> String {
    CORPUS
        .lines()
        .map(|l| serde_json::from_str::<Json>(l).unwrap())
        .find(|c| c["id"] == id)
        .unwrap()["hcl"]
        .as_str()
        .unwrap()
        .to_string()
}

/// A plan's broker command line, read for what it means: the same meaning is the same broker, whichever edition's
/// flags say it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Meaning {
    data_dir: String,
    major: String,
    advertise: String,
    console: bool,
    registry: bool,
    auto_create: bool,
    default_partitions: i64,
    retention_ms: i64,
    retention_bytes: i64,
}

fn flags(args: &[Json]) -> BTreeMap<String, String> {
    let args: Vec<String> = args
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect();
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some((k, v)) = a.split_once('=') {
            out.insert(k.to_string(), v.to_string());
            i += 1;
        } else if a.starts_with("--") && i + 1 < args.len() && !args[i + 1].starts_with("--") {
            out.insert(a.clone(), args[i + 1].clone());
            i += 2;
        } else {
            out.insert(a.clone(), String::new());
            i += 1;
        }
    }
    out
}

fn number(f: &BTreeMap<String, String>, k: &str, default: i64) -> i64 {
    f.get(k).map_or(default, |v| v.parse().unwrap())
}

/// Go's `__serve` flags (`doze-kafka/module/serve.go`), with its own defaults and its reading of `--version`.
fn go_meaning(args: &[Json]) -> Meaning {
    let f = flags(args);
    assert_eq!(args[0], "__serve");
    let major = f["--version"]
        .parse::<u8>()
        .map_or("4".to_string(), |v| v.to_string());
    Meaning {
        data_dir: f["--datadir"].clone(),
        major,
        advertise: f["--advertise"].clone(),
        console: f.contains_key("--console-socket") || f.contains_key("--console-addr"),
        registry: f.contains_key("--registry-socket"),
        auto_create: f.get("--auto-create").is_none_or(|v| v == "true"),
        default_partitions: number(&f, "--default-partitions", 1),
        retention_ms: number(&f, "--retention-ms", 0),
        retention_bytes: number(&f, "--retention-bytes", 0),
    }
}

/// The Rust broker's own flags (`doze-kafka --help`).
fn rust_meaning(args: &[Json]) -> Meaning {
    let f = flags(args);
    Meaning {
        data_dir: f["--data-dir"].clone(),
        major: f["--kafka-version"].clone(),
        advertise: format!("{}:{}", f["--advertised-host"], f["--advertised-port"]),
        console: f.contains_key("--console-addr")
            && f.get("--console").is_none_or(|v| v != "false"),
        registry: f.contains_key("--schema-registry-addr"),
        auto_create: f.get("--auto-create-topics").is_none_or(|v| v == "true"),
        default_partitions: number(&f, "--default-partitions", 1),
        retention_ms: number(&f, "--retention-ms", 0),
        retention_bytes: number(&f, "--retention-bytes", 0),
    }
}

/// Cases Go accepts and the module refuses before boot, each for a listed reason (docs/port/phase11.md, "Better than
/// Go"). The generated corpus's topic blocks all carry `config = { tuning = "on" }`, which is no topic config.
const REFUSED_BY_RUST: &[(&str, &str)] = &[
    ("kafka/016-block-topic", "tuning is no topic config"),
    ("kafka/017-block-topic-two", "tuning is no topic config"),
    (
        "kafka/018-block-topic-duplicate",
        "a topic declared twice; and tuning",
    ),
    (
        "kafka/020-block-topic-with-args",
        "tuning is no topic config",
    ),
    ("kafka/101-topic-partitions-zero", "Go read 0 as 1"),
    ("kafka/102-topic-partitions-negative", "Go read -2 as 1"),
    (
        "kafka/103-topic-partitions-over-max",
        "past the broker's --max-partitions",
    ),
    (
        "kafka/104-topic-name-space",
        "Kafka refuses the name, after boot in Go",
    ),
    (
        "kafka/105-topic-name-dot",
        "Kafka refuses the name, after boot in Go",
    ),
    (
        "kafka/106-topic-name-too-long",
        "Kafka refuses the name, after boot in Go",
    ),
    (
        "kafka/108-topic-config-unknown",
        "no such topic config (did you mean retention.ms)",
    ),
    (
        "kafka/109-topic-config-bad-value",
        "cleanup.policy has no value shrink",
    ),
    (
        "kafka/111-retention-negative",
        "Go read a negative retention as unbounded",
    ),
    ("kafka/114-default-partitions-zero", "Go read 0 as 1"),
    ("kafka/115-default-partitions-negative", "Go read -3 as 1"),
    (
        "kafka/117-version-5",
        "no Kafka 5; the Go broker refused it only at start",
    ),
    ("kafka/118-version-dotted", "Go ran 3.7 as Kafka 4"),
];

/// Cases Go refuses and the module accepts: retention in days.
const ACCEPTED_BY_RUST: &[&str] = &["kafka/110-retention-days"];

/// The topics a case declares, in order.
fn declared_topics(hcl: &str) -> Vec<String> {
    hcl.lines()
        .filter_map(|l| l.trim().strip_prefix("topic \""))
        .filter_map(|l| l.split('"').next().map(String::from))
        .collect()
}

/// A case both editions plan: the same variable and address, the declared topics as objects (Go had none), and a
/// broker command line that means what Go's did, less the registry until this broker has one.
fn same_as_go(d: &oracle::Difference, registry: bool) {
    let id = d.id.as_str();
    assert_eq!(d.go["env_var"], json!("KAFKA_BROKERS"), "{id}");
    assert_eq!(d.go["env_var"], d.rust["env_var"], "{id}");
    assert_eq!(d.go["url"], d.rust["url"], "{id}");
    assert_eq!(d.go["plan_error"], d.rust["plan_error"], "{id}");
    assert_eq!(d.go["objects"], json!([]), "{id}");
    let topics: Vec<Json> = declared_topics(&case_hcl(id))
        .into_iter()
        .map(|t| json!({"kind": "topic", "name": t}))
        .collect();
    assert_eq!(d.rust["objects"], Json::Array(topics), "{id}");
    let (go, rust) = (&d.go["plan"][0], &d.rust["plan"][0]);
    assert_eq!(d.go["plan"].as_array().unwrap().len(), 1, "{id}");
    assert_eq!(d.rust["plan"].as_array().unwrap().len(), 1, "{id}");
    for k in ["name", "env_added", "after", "hooks"] {
        assert_eq!(go[k], rust[k], "{id}: {k}");
    }
    // The broker is this binary in both: Go re-executed itself, and so does the module.
    assert!(
        rust["program"].as_str().unwrap().ends_with("kafka-plugin"),
        "{id}"
    );
    let mut want = go_meaning(go["args"].as_array().unwrap());
    // The registry: Go served one on its own socket; the module plans one once the broker has it.
    want.registry = want.registry && registry;
    assert_eq!(want, rust_meaning(rust["args"].as_array().unwrap()), "{id}");
    // Ready: Go's broker listened on a socket; this one listens on TCP (D13), and ready is that port answering.
    let rust_args = flags(rust["args"].as_array().unwrap());
    assert!(go["ready"].as_str().unwrap().starts_with("socket "), "{id}");
    assert_eq!(
        rust["ready"],
        json!(format!("tcp {}", rust_args["--listen"])),
        "{id}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn answers_as_the_go_module_did_except_for_the_listed_differences() {
    let m = harness().module().await.unwrap();
    let diffs = oracle::compare(&m, "kafka", CORPUS, GOLDEN, &[]).await;
    let registry = broker_has_registry();
    let cases = CORPUS.lines().count();
    assert_eq!(cases, 43);
    // The flags differ in every planned case (the two editions' brokers spell them differently), so every case that
    // plans is a difference, and the check is of what it means.
    let (mut same_meaning, mut refused_both) = (0, 0);
    for d in &diffs {
        let id = d.id.as_str();
        let listed = REFUSED_BY_RUST.iter().any(|(c, _)| *c == id);
        let (go_refused, rust_refused) = (d.go["refused"] == true, d.rust["refused"] == true);
        if listed {
            assert!(
                !go_refused && rust_refused,
                "{id}: listed as refused by Rust only\n{d:#?}"
            );
            continue;
        }
        if ACCEPTED_BY_RUST.contains(&id) {
            assert!(go_refused && !rust_refused, "{id}\n{d:#?}");
            continue;
        }
        assert_eq!(
            go_refused, rust_refused,
            "{id} is refused by one side only:\n{d:#?}"
        );
        if go_refused {
            refused_both += 1;
            continue;
        }
        same_as_go(d, registry);
        same_meaning += 1;
    }
    // Every case is accounted for, and the counts are the ones the phase notes give. A case both sides refuse compares
    // equal (the wording is not compared), so it is not among the differences: those are the three Go refuses and the
    // module refuses too (a topic with no name, an unknown argument, a list for a string).
    let refused_by_go: Vec<String> = GOLDEN
        .lines()
        .map(|l| serde_json::from_str::<Json>(l).unwrap())
        .filter(|g| g.get("decode_error").is_some())
        .map(|g| g["id"].as_str().unwrap().to_string())
        .filter(|id| !ACCEPTED_BY_RUST.contains(&id.as_str()))
        .collect();
    assert_eq!(
        refused_by_go,
        [
            "kafka/019-block-topic-",
            "kafka/021-unknown-arg",
            "kafka/022-wrong-type-retention"
        ]
    );
    assert!(diffs.iter().all(|d| !refused_by_go.contains(&d.id)));
    assert_eq!(refused_both, 0);
    assert_eq!(diffs.len(), cases - refused_by_go.len());
    assert_eq!(REFUSED_BY_RUST.len(), 17);
    assert_eq!(same_meaning, cases - 3 - 17 - 1);
}

#[tokio::test(flavor = "current_thread")]
async fn the_listeners_are_the_go_modules_less_the_registry_until_the_broker_has_one() {
    let m = harness().module().await.unwrap();
    let golden: Json = serde_json::from_str(GOLDEN.lines().next().unwrap()).unwrap();
    let parsed = hcl::parse_block(&case_hcl("kafka/000-example"), &m.manifest.schema).unwrap();
    let inst = doze_module_proto::msg::instance::Instance {
        name: parsed.name,
        engine: parsed.engine,
        version: parsed.version,
        data_dir: "/tmp/dzo/d/events".into(),
        socket_dir: "/tmp/dzo/s/events".into(),
        spec: parsed.spec,
        host: "events.shop.doze".into(),
        listeners: BTreeMap::new(),
        deps: vec![],
        toolchain: None,
    };
    let got: Vec<Json> = m
        .network(&inst)
        .await
        .unwrap()
        .into_iter()
        .map(|l| {
            let routes = if l.routes.is_empty() {
                Json::Null
            } else {
                json!(l.routes)
            };
            json!({"name": l.name, "port": l.port, "http": l.http, "routes": routes})
        })
        .collect();
    let mut want: Vec<Json> = golden["network"].as_array().unwrap().clone();
    if !broker_has_registry() {
        want.retain(|l| l["name"] != "registry");
    }
    assert_eq!(got, want);
    // What the module offers: everything Go's did, and objects; never prune, so no topic is deleted for being left out.
    let c = m.manifest.caps;
    for cap in [
        caps::CONVERGE,
        caps::OBJECTS,
        caps::HEALTH,
        caps::ADMIN,
        caps::CONSOLE,
    ] {
        assert_ne!(c & cap, 0, "capability {cap:#x}");
    }
    assert_eq!(c & caps::PRUNE, 0);
    assert_eq!(m.manifest.default_version.as_deref(), Some("4"));
    assert_eq!(m.manifest.versions, ["1", "2", "3", "4"]);
}

// ---------------------------------------------------------------------------------------------------------------------
// On the running broker.

fn backend(b: &Booted) -> String {
    match b.backend() {
        Some(Address::Tcp(a)) => a,
        other => panic!("a TCP backend, got {other:?}"),
    }
}

fn kafka_call<Q: Encodable, A: Decodable>(addr: &str, key: ApiKey, version: i16, q: &Q) -> A {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let payload = request_payload(key, version, 7, Some("module-test"), q);
    s.write_all(&framed(&payload)).unwrap();
    let mut len = [0; 4];
    s.read_exact(&mut len).unwrap();
    let mut body = vec![0; u32::from_be_bytes(len) as usize];
    s.read_exact(&mut body).unwrap();
    let mut body = Bytes::from(body);
    ResponseHeader::decode(&mut body, key.response_header_version(version)).unwrap();
    let a = A::decode(&mut body, version).unwrap();
    assert!(!body.has_remaining());
    a
}

fn name(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// A topic's own configs, as the broker reports them.
fn topic_configs(addr: &str, topic: &str) -> BTreeMap<String, String> {
    let mut r = DescribeConfigsResource::default();
    r.resource_type = 2;
    r.resource_name = name(topic);
    r.configuration_keys = None;
    let mut q = DescribeConfigsRequest::default();
    q.resources.push(r);
    let a: DescribeConfigsResponse = kafka_call(addr, ApiKey::DescribeConfigs, 1, &q);
    a.results[0]
        .configs
        .iter()
        .filter(|c| c.config_source == 1)
        .map(|c| {
            (
                c.name.to_string(),
                c.value.clone().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

/// Sets a topic's own configs to `configs`, as another tool would.
fn set_topic_configs(addr: &str, topic: &str, configs: &BTreeMap<String, String>) {
    use kafka_protocol::messages::alter_configs_request::{AlterConfigsResource, AlterableConfig};
    let mut r = AlterConfigsResource::default();
    r.resource_type = 2;
    r.resource_name = name(topic);
    for (k, v) in configs {
        let mut c = AlterableConfig::default();
        c.name = name(k);
        c.value = Some(name(v));
        r.configs.push(c);
    }
    let mut q = kafka_protocol::messages::AlterConfigsRequest::default();
    q.resources.push(r);
    let a: kafka_protocol::messages::AlterConfigsResponse =
        kafka_call(addr, ApiKey::AlterConfigs, 0, &q);
    assert_eq!(a.responses[0].error_code, 0);
}

/// Asks for a topic by name, letting the broker create it if it auto-creates; the error code.
fn ask_for(addr: &str, topic: &str) -> i16 {
    let mut t = MetadataRequestTopic::default();
    t.name = Some(TopicName(name(topic)));
    let mut q = MetadataRequest::default();
    q.topics = Some(vec![t]);
    q.allow_auto_topic_creation = true;
    let a: MetadataResponse = kafka_call(addr, ApiKey::Metadata, 4, &q);
    a.topics[0].error_code
}

/// Commits `offset` for a group that has no members (as a tool that sets offsets does).
fn commit(addr: &str, group: &str, topic: &str, offset: i64) {
    let mut p = OffsetCommitRequestPartition::default();
    p.partition_index = 0;
    p.committed_offset = offset;
    let mut t = OffsetCommitRequestTopic::default();
    t.name = TopicName(name(topic));
    t.partitions.push(p);
    let mut q = OffsetCommitRequest::default();
    q.group_id = GroupId(name(group));
    q.generation_id_or_member_epoch = -1;
    q.topics.push(t);
    let a: OffsetCommitResponse = kafka_call(addr, ApiKey::OffsetCommit, 2, &q);
    assert_eq!(a.topics[0].partitions[0].error_code, 0);
}

async fn resources(b: &Booted) -> Vec<Resource> {
    b.module.resources(&b.instance).await.unwrap()
}

fn find<'a>(rs: &'a [Resource], kind: &str, name: &str) -> &'a Resource {
    rs.iter()
        .find(|r| r.kind == kind && r.name == name)
        .unwrap_or_else(|| panic!("no {kind} {name} in {rs:#?}"))
}

fn graceful(stops: &Stops) {
    assert!(
        !stops.is_empty()
            && stops
                .iter()
                .all(|(_, r)| matches!(r, Ok(Stopped::Graceful(st)) if st.success())),
        "{stops:?}"
    );
}

fn with(b: &Booted) -> WithToolchain {
    WithToolchain {
        instance: b.instance.clone(),
        toolchain: b.toolchain.clone(),
    }
}

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
