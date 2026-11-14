use std::collections::BTreeMap;
use std::ffi::OsString;

use doze_module::proto::msg::instance::{Address, Granted, Instance};
use doze_module::proto::msg::value::{Node, PathSeg, Span, Value};

use super::check::{bad_topic_name, check, major, retention_ms};
use super::client::Fetched;
use super::{ConfigValue, KafkaSpec, Topic, admin, broker_args, is_module, listeners};

fn args(v: &[&str]) -> Vec<OsString> {
    v.iter().map(OsString::from).collect()
}

#[test]
fn it_is_the_module_under_its_release_name_or_when_asked_and_the_broker_otherwise() {
    assert!(is_module(&args(&["/x/bin/kafka-plugin"])));
    assert!(is_module(&args(&["kafka-plugin", "--describe"])));
    assert!(is_module(&args(&["C:/x/kafka-plugin.exe"])));
    assert!(is_module(&args(&["doze-kafka", "module"])));
    assert!(is_module(&args(&["doze-kafka", "module", "--describe"])));
    // What the plan starts: the same binary with the broker's flags.
    assert!(!is_module(&args(&[
        "/x/bin/kafka-plugin",
        "--listen",
        "127.0.0.1:1"
    ])));
    assert!(!is_module(&args(&["doze-kafka"])));
    assert!(!is_module(&args(&["doze-kafka", "--describe"])));
    assert!(!is_module(&args(&[
        "doze-kafka",
        "--listen",
        "127.0.0.1:1"
    ])));
}

#[test]
fn the_listeners_are_the_go_modules_with_the_registry_only_when_the_broker_has_one() {
    let names = |v: Vec<doze_module::proto::msg::instance::Listener>| {
        v.into_iter()
            .map(|l| (l.name, l.port, l.http, l.routes, l.tcp))
            .collect::<Vec<_>>()
    };
    let main = ("main".to_string(), 0, false, vec![], true);
    let console = (
        "console".to_string(),
        9093,
        true,
        vec!["@".to_string()],
        true,
    );
    let registry = (
        "registry".to_string(),
        8081,
        true,
        vec!["registry".to_string()],
        true,
    );
    assert_eq!(
        names(listeners(true, true)),
        vec![main.clone(), console.clone(), registry]
    );
    assert_eq!(names(listeners(true, false)), vec![main.clone(), console]);
    assert_eq!(names(listeners(false, false)), vec![main]);
    assert!(doze_module::proto::network::validate(&listeners(true, true)).is_empty());
}

fn spec() -> KafkaSpec {
    KafkaSpec {
        auto_create_topics: true,
        default_partitions: 1,
        retention: None,
        retention_bytes: 0,
        topic: vec![],
    }
}

fn topic(name: &str, partitions: i32, config: &[(&str, &str)]) -> Topic {
    Topic {
        name: name.into(),
        partitions,
        config: config
            .iter()
            .map(|(k, v)| (k.to_string(), ConfigValue(v.to_string())))
            .collect(),
    }
}

fn granted(addr: &str, backend: &str, hosts: &[&str]) -> Granted {
    Granted {
        addr: addr.into(),
        url: String::new(),
        hosts: hosts.iter().map(ToString::to_string).collect(),
        backend: Address::Tcp(backend.into()),
    }
}

fn instance(version: &str) -> Instance {
    Instance {
        name: "events".into(),
        engine: "kafka".into(),
        version: version.into(),
        data_dir: "/d/events".into(),
        socket_dir: "/s/events".into(),
        spec: Node {
            span: Span::default(),
            value: Value::Map(BTreeMap::new()),
        },
        host: "events.shop.doze".into(),
        listeners: BTreeMap::from([
            (
                "main".to_string(),
                granted("events.shop.doze:9092", "127.0.0.1:40001", &[]),
            ),
            (
                "console".to_string(),
                granted("127.0.0.1:40002", "127.0.0.1:40003", &["events.shop.doze"]),
            ),
            (
                "registry".to_string(),
                granted(
                    "127.0.0.1:40004",
                    "127.0.0.1:40005",
                    &["registry.events.shop.doze"],
                ),
            ),
        ]),
        deps: vec![],
        toolchain: None,
    }
}

#[test]
fn the_broker_listens_where_doze_grants_and_advertises_the_address_clients_dial() {
    let a = broker_args(&instance(""), &spec(), true, false).unwrap();
    assert_eq!(
        a,
        [
            "--listen",
            "127.0.0.1:40001",
            "--data-dir",
            "/d/events",
            "--kafka-version",
            "4",
            "--advertised-host",
            "events.shop.doze",
            "--advertised-port",
            "9092",
            "--console-addr",
            "127.0.0.1:40003",
            "--console-allow-host",
            "events.shop.doze",
        ]
    );
}

#[test]
fn the_block_becomes_the_brokers_flags_and_defaults_are_left_to_it() {
    let mut s = spec();
    s.auto_create_topics = false;
    s.default_partitions = 6;
    s.retention = Some("7d".into());
    s.retention_bytes = 1 << 30;
    let a = broker_args(&instance("2"), &s, true, false).unwrap();
    let tail: Vec<&str> = a.iter().map(String::as_str).skip(14).collect();
    assert_eq!(
        tail,
        [
            "--auto-create-topics=false",
            "--default-partitions",
            "6",
            "--retention-ms",
            "604800000",
            "--retention-bytes",
            "1073741824",
        ]
    );
    assert!(a.windows(2).any(|w| w == ["--kafka-version", "2"]));
    // Unbounded is the broker's default: nothing is passed for it.
    let mut s = spec();
    s.retention = Some("0s".into());
    s.retention_bytes = -1;
    assert_eq!(
        broker_args(&instance(""), &s, true, false).unwrap().len(),
        14
    );
}

#[test]
fn without_a_console_none_is_served_and_the_registry_follows_its_grant() {
    let a = broker_args(&instance(""), &spec(), false, false).unwrap();
    assert!(a.contains(&"--console=false".to_string()));
    assert!(!a.iter().any(|x| x.starts_with("--console-addr")));
    let a = broker_args(&instance(""), &spec(), true, true).unwrap();
    assert!(
        a.windows(2)
            .any(|w| w == ["--schema-registry-addr", "127.0.0.1:40005"])
    );
    let mut no_registry = instance("");
    no_registry.listeners.remove("registry");
    let a = broker_args(&no_registry, &spec(), true, true).unwrap();
    assert!(a.contains(&"--schema-registry=false".to_string()));
}

#[test]
fn a_socket_backend_is_refused_because_the_broker_listens_on_tcp() {
    let mut i = instance("");
    if let Some(g) = i.listeners.get_mut("main") {
        g.backend = Address::Unix("/s/events/main.sock".into());
    }
    let e = broker_args(&i, &spec(), true, false).unwrap_err();
    assert!(
        e.message.contains("cannot listen on a socket"),
        "{}",
        e.message
    );
}

#[test]
fn a_version_is_a_kafka_major_from_1_to_4() {
    for v in ["1", "2", "3", "4"] {
        assert_eq!(major(v).unwrap().to_string(), v);
    }
    assert_eq!(major("").unwrap(), 4);
    let d = major("3.7").unwrap_err();
    assert!(d.hint.unwrap().contains("version = 3"));
    assert!(major("5").is_err());
    assert!(major("four").is_err());
}

#[test]
fn a_retention_is_a_go_duration_or_whole_days_and_never_negative() {
    assert_eq!(retention_ms("168h"), Ok(604_800_000));
    assert_eq!(retention_ms("7d"), Ok(604_800_000));
    assert_eq!(retention_ms("1h30m"), Ok(5_400_000));
    assert_eq!(retention_ms("0s"), Ok(0));
    assert!(retention_ms("-1h").is_err());
    assert!(retention_ms("7 days").is_err());
    assert!(retention_ms("d").is_err());
    assert!(retention_ms("99999999999999999999d").is_err());
}

#[test]
fn a_topic_name_is_what_kafka_takes() {
    assert_eq!(bad_topic_name("users.signed-up_v2"), None);
    assert!(bad_topic_name("has space").is_some());
    assert!(bad_topic_name(".").is_some());
    assert!(bad_topic_name("..").is_some());
    assert!(bad_topic_name(&"a".repeat(249)).is_none());
    assert!(bad_topic_name(&"a".repeat(250)).is_some());
    assert!(bad_topic_name("ünïcode").is_some());
}

fn paths(spec: &KafkaSpec, version: &str) -> Vec<(Vec<PathSeg>, String)> {
    check(spec, version)
        .into_iter()
        .map(|d| (d.path, d.message))
        .collect()
}

#[test]
fn what_the_broker_would_refuse_is_refused_at_its_line() {
    assert!(check(&spec(), "4").is_empty());
    let mut s = spec();
    s.default_partitions = 0;
    s.retention = Some("-1h".into());
    s.retention_bytes = -5;
    s.topic = vec![
        topic(
            "ok",
            3,
            &[("cleanup.policy", "compact"), ("retention.ms", "86400000")],
        ),
        topic("has space", 0, &[]),
        topic("typo", 1, &[("retention.msec", "1")]),
        topic("bad", 1, &[("cleanup.policy", "shrink")]),
    ];
    let found = paths(&s, "5");
    let key = |k: &str| PathSeg::Key(k.into());
    let at = |i: u32, k: &str| vec![key("topic"), PathSeg::Index(i), key(k)];
    let wanted = [
        vec![],
        vec![key("default_partitions")],
        vec![key("retention")],
        vec![key("retention_bytes")],
        at(1, "name"),
        at(1, "partitions"),
        vec![
            key("topic"),
            PathSeg::Index(2),
            key("config"),
            key("retention.msec"),
        ],
        vec![
            key("topic"),
            PathSeg::Index(3),
            key("config"),
            key("cleanup.policy"),
        ],
    ];
    let got: Vec<Vec<PathSeg>> = found.iter().map(|(p, _)| p.clone()).collect();
    assert_eq!(got, wanted, "{found:#?}");
    let typo = check(&s, "4")
        .into_iter()
        .find(|d| d.path.last() == Some(&key("retention.msec")))
        .unwrap();
    assert_eq!(typo.hint.as_deref(), Some("did you mean \"retention.ms\"?"));
}

#[test]
fn a_pair_of_configs_that_conflict_is_refused_on_the_topic() {
    let mut s = spec();
    s.topic = vec![topic(
        "t",
        1,
        &[
            ("min.compaction.lag.ms", "10"),
            ("max.compaction.lag.ms", "5"),
        ],
    )];
    let found = check(&s, "4");
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].message.contains("min.compaction.lag.ms"));
}

#[test]
fn a_tail_row_has_the_record_cut_to_its_size_on_a_character() {
    let r = Fetched {
        partition: 2,
        offset: 41,
        timestamp: 86_400_000 + 3_723_000,
        key: b"k".to_vec(),
        value: "é".repeat(200).into_bytes(),
    };
    let row = admin::row(&r);
    assert_eq!(row.cells["time"], "01:02:03");
    assert_eq!(row.cells["partition"], "2");
    assert_eq!(row.cells["offset"], "41");
    assert_eq!(row.cells["key"], "k");
    let v = &row.cells["value"];
    assert!(
        v.ends_with('…') && v.len() <= 240 + '…'.len_utf8(),
        "{}",
        v.len()
    );
}

#[test]
fn the_console_is_the_go_modules_three_panels_and_the_web_console() {
    let c = admin::console();
    let ids: Vec<&str> = c.panels.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["topics", "groups", "tail"]);
    assert_eq!(c.front.as_deref(), Some("console"));
    // A build without the console serves none, so the app is not told of one.
    let web = cfg!(feature = "console").then_some("http://{address:console}");
    assert_eq!(c.web_url.as_deref(), web);
    let a = admin::actions();
    assert_eq!(a.len(), 1);
    assert_eq!((a[0].id.as_str(), a[0].kind.as_str()), ("produce", "topic"));
}

/// The console and actions as the app's fixture takes them (`apps/doze-app/fixtures/consoles.ts`, type `Spec` and
/// `Action`): only the fields that are set.
fn console_json() -> serde_json::Value {
    use serde_json::{Map, Value, json};
    let c = admin::console();
    let panels: Vec<Value> = c
        .panels
        .iter()
        .map(|p| {
            let mut o = Map::new();
            o.insert("id".into(), json!(p.id));
            o.insert("title".into(), json!(p.title));
            o.insert("kind".into(), json!(p.kind));
            if !p.source.is_empty() {
                o.insert("source".into(), json!(p.source));
            }
            let columns: Vec<Value> = p
                .columns
                .iter()
                .map(|col| json!({"key": col.key, "title": col.title, "role": col.role, "align": col.align}))
                .collect();
            o.insert("columns".into(), Value::Array(columns));
            if let Some(s) = &p.select {
                o.insert("select".into(), json!(s));
            }
            Value::Object(o)
        })
        .collect();
    let actions: Vec<Value> = admin::actions()
        .iter()
        .map(|a| {
            json!({"id": a.id, "label": a.label, "kind": a.kind, "destructive": a.destructive, "input_hint": a.input_hint})
        })
        .collect();
    json!({
        "spec": {"front": c.front, "web_url": c.web_url, "panels": panels, "tabs": [], "extend": []},
        "actions": actions,
    })
}

/// The kafka module's console and actions, pinned: the app's fixture for a kafka instance is regenerated from
/// `console.snapshot.json`, never written by hand. `DOZE_UPDATE_SNAPSHOT=1` rewrites it after a deliberate change.
#[test]
#[cfg(feature = "console")]
fn the_console_and_actions_match_their_snapshot() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/module/console.snapshot.json"
    );
    let now = serde_json::to_string_pretty(&console_json()).unwrap() + "\n";
    if std::env::var_os("DOZE_UPDATE_SNAPSHOT").is_some() {
        std::fs::write(path, &now).unwrap();
    }
    let pinned = std::fs::read_to_string(path).unwrap_or_default();
    assert_eq!(
        pinned, now,
        "the console changed: review it, then DOZE_UPDATE_SNAPSHOT=1"
    );
}
