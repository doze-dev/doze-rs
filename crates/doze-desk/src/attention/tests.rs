//! Go's attention tests (`view/attention_test.go`, `serve/attention_test.go`), each held as it was.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::away::{away_of, span};
use super::*;
use crate::conn::Conn;
use crate::page::time::Zone;
use crate::ports::Holder;
use crate::view::StackView;
use doze_control::{Event, InstanceView, Timestamp};
use doze_projects::StackKey;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Go's `t0`: 2026-11-06 14:00 UTC.
fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_793_973_600)
}

fn ts(t: SystemTime) -> Timestamp {
    Timestamp::from_system_time(t)
}

fn running(insts: serde_json::Value) -> StackView {
    let mut st = StackView::new(StackKey::new("/h/projects/shop-1"));
    (st.name, st.dir, st.running) = ("shop".into(), "/code/shop".into(), true);
    st.link.conn = Conn::Live;
    st.instances = Arc::new(serde_json::from_value::<Vec<InstanceView>>(insts).unwrap());
    st
}

fn ev(t: SystemTime, kind: &str, inst: &str) -> Event {
    serde_json::from_value(json!({"seq": 1, "time": ts(t), "kind": kind, "instance": inst}))
        .unwrap()
}

#[test]
fn a_crash_names_the_port_holder_and_its_print_ignores_the_try() {
    let mut st = running(
        json!([{"name": "api", "kind": "app", "state": "crashing", "last_error": "listen tcp 127.0.0.1:8080: bind: address already in use", "restart_count": 3, "restart_max": 5}]),
    );
    let held = |p: u16| {
        Some(Holder {
            port: p,
            pid: 4121,
            command: "node".into(),
            doze: false,
            stack: String::new(),
            service: String::new(),
        })
    };
    let f = StackFacts {
        held: Some(&held),
        ..StackFacts::default()
    };
    let items = stack_items(&st, &f, t0());
    assert_eq!(items.len(), 1);
    let it = &items[0];
    assert!(it.red && it.title == "api keeps crashing");
    assert_eq!(
        it.detail,
        "port 8080 is held by node (pid 4121) · try 3 of 5"
    );
    assert_eq!(
        (
            it.fixes[0].kind,
            it.fixes[0].pid,
            it.fixes[0].label.as_str()
        ),
        (FixKind::Holder, 4121, "Stop pid 4121 and start api")
    );
    let mut insts = (*st.instances).clone();
    insts[0].restart_count = 4;
    st.instances = Arc::new(insts);
    assert_eq!(stack_items(&st, &f, t0())[0].print, it.print);
}

#[test]
fn a_failed_task_and_a_changed_file() {
    let st = running(json!([
        {"name": "migrate", "kind": "task", "task": {"result": "failed", "exit_code": 1, "ran_at": ts(t0())}, "last_error": "relation \"orders\" does not exist"},
        {"name": "db", "kind": "module", "state": "active"}
    ]));
    let ch = crate::overview::ConfigChanged {
        text: "doze.hcl changed 4m ago".into(),
    };
    let items = stack_items(
        &st,
        &StackFacts {
            changed: Some(&ch),
            ..StackFacts::default()
        },
        t0(),
    );
    assert_eq!(items.len(), 2);
    assert_eq!(
        (
            items[0].kind,
            items[0].title.as_str(),
            items[0].fixes[0].verb
        ),
        ("task", "migrate failed", "task")
    );
    assert_eq!(
        (items[1].kind, items[1].red, items[1].fixes[0].verb),
        ("changed", false, "restart")
    );
}

#[test]
fn a_missing_folder_says_only_that() {
    let mut st = StackView::new(StackKey::new("k"));
    (st.name, st.dir) = ("playground".into(), "/tmp/playground".into());
    let c = crate::overview::ConfigError {
        summary: "x".into(),
        ..Default::default()
    };
    let items = stack_items(
        &st,
        &StackFacts {
            missing: "the folder is gone",
            config: Some(&c),
            ..StackFacts::default()
        },
        t0(),
    );
    assert_eq!(items.len(), 1);
    assert_eq!(
        (items[0].title.as_str(), items[0].fixes[0].verb),
        ("playground's folder is gone", "remove")
    );
}

#[test]
fn a_daemon_down_and_a_failed_start() {
    let mut st = running(json!([]));
    (st.link.conn, st.link.last_frame) = (Conn::Down, Some(t0() - Duration::from_secs(120)));
    let fail = StackFailure {
        what: "Restarting shop failed".into(),
        lines: vec!["exit 1".into()],
        ..Default::default()
    };
    let items = stack_items(
        &st,
        &StackFacts {
            failed: Some(&fail),
            ..StackFacts::default()
        },
        t0(),
    );
    assert_eq!(
        (items.len(), items[0].kind, items[1].kind, items[1].red),
        (2, "failed", "daemon", true)
    );
    // A running stack's failure cannot be tried again from here: only dismissed.
    assert_eq!(
        (items[0].fixes.len(), items[0].fixes[0].kind),
        (1, FixKind::Dismiss)
    );
    assert!(items[0].mono && items[0].detail == "exit 1");
}

#[test]
fn attention_sorts_counts_and_hides() {
    let it = |id: &str, print: &str, red, fixed| Item {
        id: id.into(),
        print: print.into(),
        red,
        fixed,
        ..Item::default()
    };
    let items = vec![
        it("y1", "a", false, false),
        it("r1", "b", true, false),
        it("r2", "c", true, false),
        it("g", "d", true, true),
    ];
    let nn = [
        ("r2".to_owned(), "c".to_owned()),
        ("y1".to_owned(), "old".to_owned()),
    ]
    .into_iter()
    .collect();
    let a = attention_of(items, &nn);
    assert_eq!((a.red, a.yellow, a.hidden), (1, 1, 1));
    let order: Vec<&str> = a.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(order, ["r1", "y1", "g"]);
}

#[test]
fn machine_items_group_by_section() {
    let part = |sec: &str, name: &str, ok, fix: &str| MachinePart {
        section: sec.into(),
        name: name.into(),
        ok,
        text: String::new(),
        fix: fix.into(),
    };
    let parts = [
        part("names", "resolver route", true, ""),
        part("names", "network", false, "doze dns-setup"),
        part("domains", "route dev.acme.com", false, "doze trust"),
    ];
    let items = machine_items(&parts, false, 3, 0);
    assert_eq!(items.len(), 3);
    assert_eq!(
        (items[0].fixes[0].kind, items[0].fixes[0].text.as_str()),
        (FixKind::Copy, "doze dns-setup")
    );
    assert_eq!(
        (items[2].title.as_str(), items[2].fixes[0].kind),
        ("3 things left behind", FixKind::Clean)
    );
    let got = machine_items(&parts, true, 0, 2);
    assert_eq!(
        (got[0].fixes[0].kind, got.last().unwrap().id.as_str()),
        (FixKind::Repair, "leftdata")
    );
}

#[test]
fn away_says_what_happened_while_the_window_was_closed() {
    let closed = t0() - Duration::from_secs(3 * 3600 + 12 * 60);
    let mut st = running(json!([
        {"name": "api", "kind": "app", "state": "active"},
        {"name": "migrate", "kind": "task", "task": {"result": "failed", "exit_code": 1, "ran_at": ts(t0() - Duration::from_secs(20 * 60))}}
    ]));
    st.events = Arc::new(vec![
        ev(closed - Duration::from_secs(60), "crash", "api"), // before: not news
        ev(t0() - Duration::from_secs(90 * 60), "crash", "api"),
        ev(t0() - Duration::from_secs(80 * 60), "crash", "api"),
        ev(t0() - Duration::from_secs(70 * 60), "wake", "api"),
    ]);
    let a = away_of(std::slice::from_ref(&st), closed, t0(), Zone::UTC).unwrap();
    assert_eq!((a.r#for.as_str(), a.events.len()), ("3 h 12 m", 2));
    assert_eq!(
        (a.events[0].instance.as_str(), a.events[0].tone),
        ("migrate", "bad")
    );
    assert_eq!(a.events[1].what, "crashed twice in shop and started again");
    assert_eq!(a.events[1].at, "12:30, 12:40");
    assert!(away_of(&[running(json!([]))], closed, t0(), Zone::UTC).is_none());
    assert_eq!(
        (
            span(Duration::from_secs(40 * 60)),
            span(Duration::from_secs(7200)),
            span(Duration::from_secs(3 * 86_400))
        ),
        ("40 m".into(), "2 h".into(), "3 days".into())
    );
}

#[test]
fn a_silent_wake_is_stuck_and_a_busy_one_is_not() {
    let mut st = running(json!([{"name": "kafka", "kind": "module", "state": "booting"}]));
    st.events = Arc::new(vec![
        ev(t0() - Duration::from_secs(360), "wake", "kafka"),
        ev(t0() - Duration::from_secs(180), "step", "kafka"),
    ]);
    let items = stack_items(&st, &StackFacts::default(), t0());
    assert_eq!(
        (items.len(), items[0].title.as_str(), items[0].red),
        (1, "kafka is still waking", true)
    );
    let mut e = (*st.events).clone();
    e.push(ev(t0() - Duration::from_secs(10), "step", "kafka"));
    st.events = Arc::new(e);
    assert!(stack_items(&st, &StackFacts::default(), t0()).is_empty());
}

#[test]
fn a_stack_that_ended_unasked_is_red() {
    let mut st = StackView::new(StackKey::new("k"));
    (st.name, st.dir) = ("shop".into(), "/code/shop".into());
    (st.last_up, st.last_down) = (
        Some(t0() - Duration::from_secs(13 * 60)),
        Some(t0() - Duration::from_secs(36_000)),
    );
    let items = stack_items(&st, &StackFacts::default(), t0());
    assert_eq!(
        (items.len(), items[0].title.as_str(), items[0].fixes[0].verb),
        (1, "shop stopped without being asked", "up-last")
    );
    st.last_down = Some(t0() - Duration::from_secs(60)); // doze down ran after it was up
    assert!(stack_items(&st, &StackFacts::default(), t0()).is_empty());
    (st.last_down, st.last_up) = (None, Some(t0() - Duration::from_secs(10))); // a start in its first moment
    assert!(stack_items(&st, &StackFacts::default(), t0()).is_empty());
}

/// Builds the list as the app does at `now`.
fn build(
    s: &mut Steady,
    st: &StackView,
    nn: &std::collections::BTreeMap<String, String>,
    now: SystemTime,
) -> Attention {
    let items = stack_items(st, &StackFacts::default(), now);
    s.step(items, std::slice::from_ref(st), &|_| false, nn, now)
}

#[test]
fn a_crash_that_stops_is_fixed_only_after_its_grace_then_leaves() {
    for aside in [false, true] {
        let mut st = running(
            json!([{"name": "api", "kind": "app", "state": "crashing", "last_error": "boom"}]),
        );
        let (mut s, mut nn, mut now) = (Steady::default(), std::collections::BTreeMap::new(), t0());
        let a = build(&mut s, &st, &nn, now);
        let it = a.items[0].clone();
        assert!(it.red && it.id.ends_with("/api"));
        if aside {
            not_now(&mut nn, &it.id, Some(&it.print));
        }
        st.instances = Arc::new(
            serde_json::from_value(json!([{"name": "api", "kind": "app", "state": "active"}]))
                .unwrap(),
        );
        now += Duration::from_secs(1);
        let a = build(&mut s, &st, &nn, now);
        if !aside {
            assert!(
                a.items.first().is_some_and(|i| !i.fixed),
                "within its grace it stays as it was"
            );
        }
        now += GRACE_FOR + Duration::from_secs(1);
        let a = build(&mut s, &st, &nn, now);
        match a.items.first() {
            Some(i) if aside => panic!("put aside, yet shown: {i:?}"),
            None if !aside => panic!("after its grace it should say fixed"),
            Some(i) => assert!(i.fixed && i.fixes.is_empty() && a.red == 0),
            None => {}
        }
        now += FIXED_FOR + Duration::from_secs(1);
        assert!(
            build(&mut s, &st, &nn, now).items.is_empty(),
            "a fixed item leaves"
        );
        assert!(!s.waiting());
    }
}

#[test]
fn a_failed_task_outlives_its_stack() {
    let mut st = running(
        json!([{"name": "migrate", "kind": "task", "task": {"result": "failed", "exit_code": 1, "ran_at": ts(t0())}}]),
    );
    let (mut s, nn, mut now) = (Steady::default(), std::collections::BTreeMap::new(), t0());
    let task = build(&mut s, &st, &nn, now).items[0].clone();
    assert_eq!(task.kind, "task");
    st.running = false;
    st.instances = Arc::default();
    now += GRACE_FOR + Duration::from_secs(1);
    let a = build(&mut s, &st, &nn, now);
    let it = a.items.iter().find(|i| i.id == task.id).unwrap();
    assert!(!it.fixed && it.red && it.fixes[0].verb == "up-last" && it.fixes[0].label == "Up shop");
    assert!(it.detail.ends_with(" · its stack is stopped"));
    // A stack mid-action carries its items as they were.
    let a = s.step(
        Vec::new(),
        std::slice::from_ref(&st),
        &|_| true,
        &nn,
        now + Duration::from_secs(60),
    );
    assert_eq!(a.items.len(), 1);
}

#[test]
fn dismissed_items_leave_quietly() {
    let st = running(json!([{"name": "api", "kind": "app", "state": "crashing"}]));
    let (mut s, nn) = (Steady::default(), std::collections::BTreeMap::new());
    build(&mut s, &st, &nn, t0());
    s.quiet(|i| i.instance == "api");
    let healthy = running(json!([{"name": "api", "kind": "app", "state": "active"}]));
    let a = build(&mut s, &healthy, &nn, t0() + Duration::from_secs(1));
    assert!(a.items.is_empty());
}
