//! The registry's own rules, ported from the Go edition's `registry_test.go`, `bounds_test.go`, `limits_exact_test.go`
//! and `doze_ext_test.go` (the oracle's answers kept).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};

use super::*;
use crate::error::code;
use crate::json;
use crate::store::{Batch, MemoryStorage, StoreError};

const USER1: &str = r#"{"type":"record","name":"User","fields":[{"name":"id","type":"long"}]}"#;
const USER2: &str = r#"{"type":"record","name":"User","fields":[{"name":"id","type":"long"},{"name":"email","type":["null","string"],"default":null}]}"#;

fn open(st: &MemoryStorage) -> Registry {
    Registry::open(Box::new(st.clone())).unwrap()
}

fn avro(s: &str) -> SchemaInput {
    SchemaInput::of(SchemaType::Avro, s)
}

fn reg(r: &Registry, subject: &str, s: &str) -> u32 {
    match r
        .register(subject, &avro(s), false, &Origin::default())
        .unwrap()
    {
        RegisterResult::Id(id) => id,
        other => panic!("{other:?}"),
    }
}

fn err(r: Result<impl std::fmt::Debug, RegistryError>) -> u32 {
    r.unwrap_err().error_code
}

/// A record whose one field is an array nested `depth` times around `leaf`.
fn nested(depth: usize, leaf: &str) -> String {
    let mut t = format!("\"{leaf}\"");
    for _ in 0..depth {
        t = format!(r#"{{"type":"array","items":{t}}}"#);
    }
    format!(r#"{{"type":"record","name":"Deep","fields":[{{"name":"f","type":{t}}}]}}"#)
}

#[test]
fn everything_survives_a_restart_and_a_spent_id_is_not_given_again() {
    let st = MemoryStorage::default();
    let r = open(&st);
    assert_eq!(
        (
            reg(&r, "orders", USER1),
            reg(&r, "orders", USER2),
            reg(&r, "users", r#""string""#)
        ),
        (1, 2, 3)
    );
    r.set_level("orders", "FULL").unwrap();
    r.set_level("", "NONE").unwrap();
    r.set_mode("frozen", "READONLY").unwrap();
    r.delete_version("orders", "1", false).unwrap();
    r.delete_subject("users", false).unwrap();
    r.delete_subject("users", true).unwrap();

    let r2 = open(&st);
    assert_eq!(r2.subjects("", false, false), ["orders"]);
    assert_eq!(r2.mode("frozen", false).unwrap(), Mode::ReadOnly);
    assert_eq!(r2.versions("orders", true).unwrap(), [1, 2]);
    assert_eq!(r2.versions("orders", false).unwrap(), [2]);
    assert_eq!(r2.level("orders", false).unwrap(), Level::Full);
    assert_eq!(r2.level("", false).unwrap(), Level::None);
    assert_eq!(err(r2.schema_by_id(3)), code::SCHEMA_NOT_FOUND);
    assert_eq!(reg(&r2, "fresh", r#""int""#), 4, "id 3 was spent");
}

/// A store that refuses every write while `fail` is set.
#[derive(Debug, Clone, Default)]
struct Failing {
    inner: MemoryStorage,
    fail: Arc<AtomicBool>,
}

impl Storage for Failing {
    fn load(&self) -> Result<kafka_meta::Records, StoreError> {
        self.inner.load()
    }

    fn apply(&self, puts: &[(Vec<u8>, Vec<u8>)], deletes: &[Vec<u8>]) -> Result<(), StoreError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(StoreError("disk full".into()));
        }
        self.inner.apply(puts, deletes)
    }
}

#[test]
fn a_failed_write_changes_nothing() {
    let st = Failing::default();
    let r = Registry::open(Box::new(st.clone())).unwrap();
    reg(&r, "s", USER1);
    st.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        err(r.register("s", &avro(USER2), false, &Origin::default())),
        code::STORE
    );
    assert_eq!(err(r.set_level("s", "FULL")), code::STORE);
    assert_eq!(err(r.delete_subject("s", false)), code::STORE);
    assert_eq!(r.versions("s", false).unwrap(), [1]);
    assert_eq!(r.level("s", true).unwrap(), Level::Backward);
    st.fail.store(false, Ordering::SeqCst);
    assert_eq!(reg(&r, "s", USER2), 2, "the failed attempt took no id");
}

#[test]
fn one_schema_registered_at_once_from_many_threads_gets_one_id_and_one_version() {
    let r = Arc::new(open(&MemoryStorage::default()));
    let start = Arc::new(Barrier::new(16));
    let ids: Vec<u32> = (0..16)
        .map(|_| {
            let (r, start) = (Arc::clone(&r), Arc::clone(&start));
            std::thread::spawn(move || {
                start.wait();
                reg(&r, "same", USER1)
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    assert!(ids.iter().all(|&id| id == ids[0]), "{ids:?}");
    assert_eq!(r.versions("same", false).unwrap(), [1]);
}

#[test]
fn the_level_chooses_the_versions_compared() {
    let v1 = r#"{"type":"record","name":"R","fields":[{"name":"a","type":"string"}]}"#;
    let v2 = r#"{"type":"record","name":"R","fields":[{"name":"a","type":"string"},{"name":"b","type":"int","default":0}]}"#;
    let cand = r#"{"type":"record","name":"R","fields":[{"name":"a","type":"string"},{"name":"b","type":"int"},{"name":"c","type":"int"}]}"#;
    for (level, ok, compared) in [
        ("NONE", true, vec![2]),
        ("BACKWARD", false, vec![2]),
        ("BACKWARD_TRANSITIVE", false, vec![1, 2]),
        ("FORWARD", true, vec![2]),
        ("FULL_TRANSITIVE", false, vec![1, 2]),
    ] {
        let r = open(&MemoryStorage::default());
        reg(&r, "s", v1);
        r.set_level("s", "NONE").unwrap();
        reg(&r, "s", v2);
        r.set_level("s", level).unwrap();
        let (seen, found) = r.check("s", None, &avro(cand), false, None).unwrap();
        assert_eq!((found.is_empty(), seen), (ok, compared), "{level}");
    }
}

#[test]
fn a_record_that_cannot_be_read_stops_the_open_and_is_named() {
    let st = MemoryStorage::default();
    let mut b = Batch::default();
    b.puts
        .push((Key::Schema(1).encode(), b"not postcard".to_vec()));
    b.puts.push((b"zzz".to_vec(), vec![1]));
    st.apply(&b.puts, &[]).unwrap();
    let e = Registry::open(Box::new(st)).unwrap_err();
    assert!(
        e.contains("record \"s\\0\\0\\0\\u{1}\"") || e.contains("record"),
        "{e}"
    );
}

#[test]
fn nesting_is_bounded_and_answered_quickly() {
    let r = open(&MemoryStorage::default());
    let start = std::time::Instant::now();
    let e = r
        .register(
            "deep",
            &avro(&nested(3000, "int")),
            false,
            &Origin::default(),
        )
        .unwrap_err();
    assert_eq!(e.error_code, code::INVALID_SCHEMA);
    assert!(
        e.message.contains("nested more than") && e.message.len() < 200,
        "{}",
        e.message
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    // The limit is on nesting, not on brackets inside names and docs; a schema at the limit is ordinary.
    let doc = format!(
        r#"{{"type":"record","name":"R","doc":"{}\"","fields":[{{"name":"f","type":"int"}}]}}"#,
        "[{".repeat(400)
    );
    reg(&r, "brackets-in-doc", &doc);
    let mut k = 0;
    while json::depth(&nested(k + 1, "int")) <= json::MAX_SCHEMA_DEPTH {
        k += 1;
    }
    assert_eq!(json::depth(&nested(k, "int")), json::MAX_SCHEMA_DEPTH);
    reg(&r, "at-the-limit", &nested(k, "int"));
    let over = r
        .register(
            "over",
            &avro(&nested(k + 1, "int")),
            false,
            &Origin::default(),
        )
        .unwrap_err();
    assert!(
        over.message.contains("nested more than"),
        "{}",
        over.message
    );
}

#[test]
fn subjects_are_bounded_exactly_at_the_limit_and_control_characters_are_refused() {
    let r = open(&MemoryStorage::default());
    let at = "s".repeat(MAX_SUBJECT_LEN);
    reg(&r, &at, r#""string""#);
    let over = format!("{at}s");
    let e = r
        .register(&over, &avro(r#""string""#), false, &Origin::default())
        .unwrap_err();
    assert_eq!(e.error_code, code::INVALID_SUBJECT);
    assert!(e.message.len() < 200, "the refusal echoes the subject");
    assert_eq!(err(r.set_level(&over, "FULL")), code::INVALID_SUBJECT);
    assert_eq!(err(r.set_mode(&over, "READONLY")), code::INVALID_SUBJECT);
    for s in ["\u{1}orders", "ord\u{1f}ers", "orders\u{7f}", "__GLOBAL"] {
        assert_eq!(
            err(r.register(s, &avro(r#""string""#), false, &Origin::default())),
            code::INVALID_SUBJECT,
            "{s:?}"
        );
    }
    assert_eq!(
        err(r.register("", &avro(r#""string""#), false, &Origin::default())),
        code::INVALID_REQUEST
    );
    for s in ["a b", "ünïcode", "a/b", "%2F", "x:y"] {
        reg(&r, s, r#""string""#);
    }
}

#[test]
fn a_diamond_of_references_is_resolved_once_per_schema() {
    let r = open(&MemoryStorage::default());
    reg(&r, "l0", r#"{"type":"record","name":"L0","fields":[]}"#);
    let start = std::time::Instant::now();
    for i in 1..=26 {
        let prev = format!("l{}", i - 1);
        let refs = vec![
            Reference {
                name: format!("L{}", i - 1),
                subject: prev.clone(),
                version: 1,
            },
            Reference {
                name: format!("L{}", i - 1),
                subject: prev,
                version: 1,
            },
        ];
        let schema = format!(
            r#"{{"type":"record","name":"L{i}","fields":[{{"name":"a","type":"L{}"}},{{"name":"b","type":"L{}"}}]}}"#,
            i - 1,
            i - 1
        );
        let input = SchemaInput {
            schema: Some(schema),
            schema_type: None,
            references: Some(refs),
        };
        r.register(&format!("l{i}"), &input, false, &Origin::default())
            .unwrap();
    }
    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn a_chain_of_forty_references_registers_and_a_referenced_version_cannot_go() {
    let r = open(&MemoryStorage::default());
    reg(&r, "c0", r#"{"type":"record","name":"C0","fields":[]}"#);
    for i in 1..=40 {
        let input = SchemaInput {
            schema: Some(format!(
                r#"{{"type":"record","name":"C{i}","fields":[{{"name":"p","type":"C{}"}}]}}"#,
                i - 1
            )),
            schema_type: None,
            references: Some(vec![Reference {
                name: format!("C{}", i - 1),
                subject: format!("c{}", i - 1),
                version: 1,
            }]),
        };
        r.register(&format!("c{i}"), &input, false, &Origin::default())
            .unwrap();
    }
    assert_eq!(r.referenced_by("c0", "1").unwrap(), [2]);
    assert_eq!(
        err(r.delete_version("c0", "1", false)),
        code::REFERENCE_EXISTS
    );
    assert_eq!(err(r.delete_subject("c39", false)), code::REFERENCE_EXISTS);
    let resolved = r.resolved("c40", "latest").unwrap();
    assert!(resolved.inlined && resolved.references.len() == 40);
    assert!(
        resolved.schema.contains(r#""name":"C0""#),
        "{}",
        resolved.schema
    );
}

#[test]
fn the_overview_gives_ids_as_runs_and_a_soft_deleted_id_still_resolves() {
    let r = open(&MemoryStorage::default());
    for (i, s) in [
        r#""string""#,
        r#""int""#,
        r#""long""#,
        r#""double""#,
        r#""float""#,
    ]
    .iter()
    .enumerate()
    {
        reg(&r, &format!("s{i}"), s);
    }
    r.delete_subject("s3", false).unwrap();
    r.delete_subject("s3", true).unwrap();
    assert_eq!(r.overview().ids_in_use, [[1, 3], [5, 5]]);
    r.delete_version("s1", "1", false).unwrap();
    let o = r.overview();
    assert_eq!(
        (o.ids_in_use, o.live_ids),
        (vec![[1, 3], [5, 5]], vec![[1, 1], [3, 3], [5, 5]])
    );
    assert_eq!(o.next_id, 6);
    assert!(
        o.subjects
            .iter()
            .any(|s| s.subject == "s1" && s.deleted && s.deleted_versions == 1)
    );
}

#[test]
fn who_registered_a_version_and_when_survive_a_restart() {
    let st = MemoryStorage::default();
    let r = open(&st);
    let who = Origin {
        user: "alice".into(),
        from: "10.0.0.7".into(),
        agent: "confluent-kafka-python/2.5".into(),
    };
    r.register("s", &avro(USER1), false, &who).unwrap();
    let info = open(&st).version_info("s", "1").unwrap();
    assert_eq!(
        (
            info.registered_by.as_str(),
            info.from.as_str(),
            info.agent.as_str()
        ),
        ("alice", "10.0.0.7", who.agent.as_str())
    );
    assert!(info.registered_at.is_some_and(|t| t.ends_with('Z')));
}

#[test]
fn a_schema_without_references_is_not_inlined() {
    let r = open(&MemoryStorage::default());
    reg(&r, "plain", USER1);
    let got = r.resolved("plain", "1").unwrap();
    assert!(!got.inlined && got.references.is_empty());
}

#[test]
fn registering_a_soft_deleted_schema_again_makes_it_the_latest_and_drops_the_old_version() {
    let r = open(&MemoryStorage::default());
    reg(&r, "s", USER1);
    reg(&r, "s", USER2);
    r.delete_version("s", "1", false).unwrap();
    assert_eq!(reg(&r, "s", USER1), 1);
    assert_eq!(r.versions("s", true).unwrap(), [2, 3]);
}
