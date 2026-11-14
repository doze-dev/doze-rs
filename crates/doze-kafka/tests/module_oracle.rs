//! The module face of doze-kafka against the Go kafka module (docs/port/phase11.md), driven as doze drives it: the binary started as
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

use std::collections::BTreeMap;

use doze_module_proto::msg::hello::caps;
use doze_module_test::{hcl, oracle};
use module_support::{CORPUS, GOLDEN, broker_has_registry, case_hcl, harness};
use serde_json::{Value as Json, json};

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
