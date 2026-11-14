//! Activity, held to Go: its own tests (`view/timeline_test.go`) ported, and the JSON Go's `ActivityOf` wrote for the
//! journals in `testdata/go/activity` (made by Go at doze 23b73d6, TZ=UTC; `cases.json` names each case's inputs).
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use doze_control::InstanceView;
use doze_desk::activity::{Activity, Ask, Ev, Span, activity_of};
use doze_desk::page::time::Zone;
use doze_desk::{StackKey, StackView};

fn now() -> SystemTime {
    // 2026-11-06T20:00:00Z, Go's `now`.
    UNIX_EPOCH + Duration::from_secs(20_763 * 86_400 + 20 * 3600)
}

fn at(h_ago: f64) -> SystemTime {
    now() - Duration::from_secs_f64(h_ago * 3600.0)
}

fn inst(name: &str, kind: &str, state: &str) -> InstanceView {
    InstanceView {
        name: name.into(),
        kind: kind.into(),
        state: state.into(),
        ..InstanceView::default()
    }
}

fn shop(running: bool) -> StackView {
    let mut st = StackView::new(StackKey::new("/k"));
    st.name = "shop".into();
    st.running = running;
    st.instances = Arc::new(vec![
        inst("api", "app", "active"),
        inst("db", "module", "reaped"),
    ]);
    st
}

fn ev(t: SystemTime, kind: &str, who: &str, cause: &str, by: &str) -> Ev {
    Ev::new(t, kind, who, (cause, by))
}

fn run(
    st: &StackView,
    events: &[Ev],
    span: Span,
    only: &str,
    ram: &HashMap<String, i64>,
) -> Activity {
    activity_of(
        st,
        events.to_vec(),
        Ask {
            span,
            inst: only,
            ram,
            now: now(),
            zone: Zone::UTC,
        },
    )
}

fn day() -> Vec<Ev> {
    let mut crash = ev(at(2.0), "crash", "api", "", "");
    crash.detail = "panic: boom".into();
    vec![
        ev(at(30.0), "wake", "api", "command", ""),
        ev(at(20.0), "wake", "db", "dependency", "api"),
        ev(at(19.0), "sleep", "db", "idle", ""),
        ev(at(12.0), "sleep", "api", "shutdown", ""),
        ev(at(12.0), "sleep", "db", "shutdown", ""),
        ev(at(10.0), "wake", "api", "command", ""),
        ev(at(5.0), "wake", "db", "connection", ""),
        ev(at(4.0), "sleep", "db", "idle", ""),
        crash,
        ev(at(1.9), "wake", "api", "restart", ""),
    ]
}

/// A day of a stack: db slept most of it and was woken by api and by a connection; api crashed once; the stack was
/// down for two hours, which is not counted as sleep.
#[test]
fn counts_sleep_while_up_and_what_woke() {
    let st = shop(true);
    let ram = HashMap::from([("db".to_owned(), 512_i64 << 20)]);
    let a = run(&st, &day(), Span::Day, "", &ram);
    assert_eq!(a.summary.up, "22h of 24h");
    let db = a.rows.iter().find(|r| r.name == "db").unwrap();
    let api = a.rows.iter().find(|r| r.name == "api").unwrap();
    // db: up 22h, awake 2h of it, asleep 20h.
    assert_eq!(db.asleep, "20h asleep · 91%");
    assert_eq!(db.wakes, 2);
    assert_eq!(db.woken_by, "1 by a connection · 1 for api");
    assert_eq!(db.freed, "~10.0 GB·h");
    assert!(a.summary.freed.starts_with("about 10.0 GB·h"));
    assert_eq!(api.crashes, 1);
    assert_eq!(a.trouble.len(), 1);
    assert!(a.trouble[0].text.contains("api"));
    assert!(!a.events[0].text.is_empty());
    assert_eq!(a.short, "");
    // One service alone; and a week, which the journal does not reach.
    let none = HashMap::new();
    let one = run(&st, &day(), Span::Day, "db", &none);
    assert_eq!(one.rows.len(), 1);
    assert_eq!(one.rows[0].name, "db");
    let week = run(&st, &day(), Span::Week, "", &none);
    assert!(
        week.short.starts_with("the journal goes back"),
        "{}",
        week.short
    );
}

/// A service awake since before the journal begins is awake, not asleep.
#[test]
fn awake_before_the_journal() {
    let st = shop(true);
    let events = vec![
        ev(
            now() - Duration::from_secs(30 * 3600),
            "wake",
            "db",
            "command",
            "",
        ),
        ev(
            now() - Duration::from_secs(29 * 3600),
            "sleep",
            "db",
            "idle",
            "",
        ),
    ];
    let a = run(&st, &events, Span::Day, "", &HashMap::new());
    for r in &a.rows {
        if r.name == "api" {
            assert!(
                r.asleep_part == 0.0 && r.never,
                "api, awake all along = {r:?}"
            );
        }
        if r.name == "db" {
            assert!(
                (r.asleep_part - 1.0).abs() < 1e-9,
                "db, asleep all day = {r:?}"
            );
        }
    }
}

/// Beyond Go: each stretch knows its wake-to-ready, and each row its typical one.
#[test]
fn wake_to_ready_is_kept_for_the_hover() {
    let st = shop(true);
    let mut ready = ev(at(4.99), "ready", "db", "", "");
    ready.took = Duration::from_millis(180);
    let events = vec![
        ev(at(5.0), "wake", "db", "connection", ""),
        ready,
        ev(at(4.0), "sleep", "db", "idle", ""),
    ];
    let a = run(&st, &events, Span::Day, "", &HashMap::new());
    let db = a.rows.iter().find(|r| r.name == "db").unwrap();
    assert_eq!(db.ready, "ready in 180ms");
    assert!(db.spans[0].took.is_some());
    assert!(db.spans[0].ready > db.spans[0].from);
}

/// A 2,000-service stack with a full journal lays out fast: the replay is linear.
#[test]
fn a_big_stack_lays_out_quickly() {
    let mut st = shop(true);
    st.instances = Arc::new(
        (0..2000)
            .map(|i| {
                inst(
                    &format!("svc-{i:04}"),
                    "app",
                    if i % 3 == 0 { "active" } else { "reaped" },
                )
            })
            .collect(),
    );
    let events: Vec<Ev> = (0..10_000)
        .map(|i| {
            let kind = if i % 2 == 0 { "wake" } else { "sleep" };
            ev(
                at(23.9 - f64::from(i) * 0.002),
                kind,
                &format!("svc-{:04}", (i / 2) % 2000),
                "connection",
                "",
            )
        })
        .collect();
    let t = std::time::Instant::now();
    let a = run(&st, &events, Span::Day, "", &HashMap::new());
    assert_eq!(a.rows.len(), 2000);
    assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
}

/// Every number within a hair (Go and Rust print a float's last digit differently), everything else exactly.
fn same(go: &serde_json::Value, rs: &serde_json::Value, at: &str) {
    use serde_json::Value::{Array, Number, Object};
    match (go, rs) {
        (Number(a), Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            assert!((a - b).abs() < 1e-9, "{at}: go {a}, rust {b}");
        }
        (Array(a), Array(b)) => {
            assert_eq!(a.len(), b.len(), "{at}: lengths");
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                same(x, y, &format!("{at}/{i}"));
            }
        }
        (Object(a), Object(b)) => {
            let keys = |m: &serde_json::Map<String, serde_json::Value>| {
                m.keys().cloned().collect::<Vec<_>>()
            };
            let (mut ka, mut kb) = (keys(a), keys(b));
            ka.sort();
            kb.sort();
            assert_eq!(ka, kb, "{at}: keys");
            for (k, v) in a {
                same(v, &b[k], &format!("{at}/{k}"));
            }
        }
        _ => assert_eq!(go, rs, "{at}"),
    }
}

/// The same journals through Go's `ActivityOf` and this port: the same JSON.
#[test]
fn matches_go_on_every_case() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/go/activity");
    let file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("cases.json")).unwrap()).unwrap();
    let now = time_of(file["now"].as_str().unwrap());
    for c in file["cases"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let mut st = StackView::new(StackKey::new("/k"));
        st.name = "shop".into();
        st.running = c["running"].as_bool().unwrap();
        st.instances = Arc::new(serde_json::from_value(c["instances"].clone()).unwrap());
        let events: Vec<Ev> = c["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| doze_journal::parse_line(e.to_string().as_bytes()))
            .filter_map(|e| Ev::from_journal(&e))
            .collect();
        let ram: HashMap<String, i64> = serde_json::from_value(c["ram"].clone()).unwrap();
        let a = activity_of(
            &st,
            events,
            Ask {
                span: Span::parse(c["span"].as_str().unwrap()),
                inst: c["inst"].as_str().unwrap(),
                ram: &ram,
                now,
                zone: Zone::UTC,
            },
        );
        let go: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join(format!("{name}.json"))).unwrap(),
        )
        .unwrap();
        // doze-display lists services as a person reads them (svc-2 before svc-10); Go compared bytes.
        let (mut go, mut rs) = (go, serde_json::to_value(&a).unwrap());
        for v in [&mut go, &mut rs] {
            v["rows"]
                .as_array_mut()
                .unwrap()
                .sort_by_key(|r| r["name"].as_str().unwrap().to_owned());
        }
        same(&go, &rs, name);
    }
}

fn time_of(s: &str) -> SystemTime {
    let e = doze_journal::parse_line(format!(r#"{{"seq":1,"time":"{s}"}}"#).as_bytes()).unwrap();
    SystemTime::from(e.time.unwrap())
}
