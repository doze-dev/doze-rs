//! Every wire field the words read is carried over. Each expected value is a full struct literal, with no
//! `..Default::default()`, so a field `doze-display` adds fails to compile here until it is converted.
#![allow(clippy::unwrap_used)]

use doze_control as wire;
use doze_desk::convert;
use doze_display as words;
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn ts(secs: u64) -> wire::Timestamp {
    wire::Timestamp::from_system_time(t(secs))
}

fn t(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_790_000_000 + secs)
}

#[allow(clippy::too_many_lines)] // WHY: one literal that sets every field of the wire type
fn wire_instance() -> wire::InstanceView {
    wire::InstanceView {
        name: "api".into(),
        kind: "app".into(),
        task: Some(wire::TaskView {
            result: "failed".into(),
            exit_code: 3,
            took_ms: 1500,
            ran_at: ts(1),
            why: "watch".into(),
        }),
        run: "go run ./cmd/api".into(),
        needs: vec!["db".into(), "cache".into()],
        engine: "process".into(),
        state: "crashing".into(),
        version: "1.2".into(),
        pid: 42,
        conns: 7,
        holders: vec![
            wire::HolderView {
                kind: "dependant".into(),
                name: "web".into(),
                count: 0,
            },
            wire::HolderView {
                kind: "connection".into(),
                name: String::new(),
                count: 3,
            },
            wire::HolderView {
                kind: "pin".into(),
                ..Default::default()
            },
            wire::HolderView {
                kind: "lease".into(),
                ..Default::default()
            },
        ],
        started_at: ts(2),
        idle_since: ts(3),
        idle_after: wire::GoDuration::from_std(Duration::from_secs(90)),
        always_on: true,
        ram: 88_080_384,
        cpu: 2.5,
        endpoint: "127.0.0.1:8080".into(),
        bind: "127.0.0.1:18080".into(),
        domain: "api.shop.doze".into(),
        domains: vec!["dev.shop.example".into()],
        https: "https://dev.shop.example".into(),
        name_only: true,
        console: "http://console".into(),
        has_console: true,
        console_front: "web".into(),
        web_url: "http://web".into(),
        name_held: "5432 by postgres".into(),
        url: "postgres://x".into(),
        resource: "arn:x".into(),
        env_var: "API_URL".into(),
        data_dir: "/data/api".into(),
        last_error: "panic: boom".into(),
        tainted: true,
        declared: true,
        disabled: true,
        paused: true,
        keep_awake: true,
        group: "apps".into(),
        restart_count: 3,
        restart_max: 5,
        next_restart: ts(4),
        children: Vec::new(),
        healthy: Some(true),
    }
}

#[test]
fn an_instance_carries_every_field_the_words_read() {
    let want = words::Instance {
        name: "api".into(),
        kind: words::Kind::App,
        task: Some(words::TaskRecord {
            outcome: words::Outcome::Failed,
            exit_code: 3,
            took: Duration::from_millis(1500),
            ran_at: Some(t(1)),
            why: "watch".into(),
        }),
        run: "go run ./cmd/api".into(),
        needs: vec!["db".into(), "cache".into()],
        engine: "process".into(),
        state: words::State::Crashing,
        version: "1.2".into(),
        holders: vec![
            words::Holder::Dependant("web".into()),
            words::Holder::Connections(3),
            words::Holder::Pin,
            words::Holder::Other,
        ],
        started_at: Some(t(2)),
        idle_since: Some(t(3)),
        idle_after: Duration::from_secs(90),
        always_on: true,
        ram: 88_080_384,
        endpoint: "127.0.0.1:8080".into(),
        bind: "127.0.0.1:18080".into(),
        domain: "api.shop.doze".into(),
        name_only: true,
        console: "http://console".into(),
        has_console: true,
        console_front: "web".into(),
        web_url: "http://web".into(),
        name_held: "5432 by postgres".into(),
        url: "postgres://x".into(),
        resource: "arn:x".into(),
        data_dir: "/data/api".into(),
        last_error: "panic: boom".into(),
        tainted: true,
        disabled: true,
        paused: true,
        keep_awake: true,
        restart_count: 3,
        restart_max: 5,
        next_restart: Some(t(4)),
    };
    assert_eq!(convert::instance(&wire_instance()), want);
}

#[test]
fn go_zero_times_and_negative_counts_read_as_none_and_zero() {
    let v = wire::InstanceView {
        state: String::new(),
        ram: -1,
        restart_count: -2,
        ..wire::InstanceView::default()
    };
    let w = convert::instance(&v);
    assert_eq!(
        (w.started_at, w.idle_since, w.next_restart),
        (None, None, None)
    );
    assert_eq!(
        (w.ram, w.restart_count, w.state, w.kind),
        (0, 0, words::State::Reaped, words::Kind::Unspecified)
    );
    let snap = convert::snapshot(Duration::from_secs(300), &[v, wire_instance()]);
    assert_eq!(snap.idle_timeout, Duration::from_secs(300));
    assert_eq!(snap.instances.len(), 2);
}

#[test]
fn an_event_carries_its_time_kind_cause_and_detail() {
    let e = wire::Event {
        seq: 9,
        time: ts(5),
        kind: "wake".into(),
        instance: "db".into(),
        cause: wire::Cause {
            kind: "dependency".into(),
            by: "api".into(),
        },
        took: wire::GoDuration::from_std(Duration::from_millis(180)),
        detail: "exit 1".into(),
    };
    let want = words::Event {
        time: t(5),
        kind: words::EventKind::Wake,
        instance: "db".into(),
        cause: words::Cause::Dependency("api".into()),
        took: Duration::from_millis(180),
        detail: "exit 1".into(),
    };
    assert_eq!(convert::event(&e), want);
    assert_eq!(convert::event(&wire::Event::default()).time, UNIX_EPOCH);
}

#[test]
fn a_console_spec_carries_its_panels_and_layout() {
    let spec = wire::ConsoleSpec {
        front: "console".into(),
        web_url: "http://x".into(),
        panels: vec![wire::ConsolePanel {
            id: "topics".into(),
            title: "Topics".into(),
            kind: "table".into(),
            source: "kafka".into(),
            columns: vec![wire::ConsoleColumn {
                key: "high".into(),
                title: "high".into(),
                align: "right".into(),
                role: "rate".into(),
            }],
            select: "groups".into(),
        }],
        tabs: vec![wire::ConsoleTab {
            id: "t".into(),
            title: "T".into(),
            body: wire::ConsoleNode {
                kind: "row".into(),
                weight: 2,
                children: vec![wire::ConsoleNode {
                    kind: "detail".into(),
                    id: "d".into(),
                    title: "query".into(),
                    weight: -1,
                    panel: "topics".into(),
                    from: "table:topics".into(),
                    text: "a note".into(),
                    actions: vec!["cancel".into()],
                    children: Vec::new(),
                }],
                ..wire::ConsoleNode::default()
            },
        }],
        extend: Vec::new(),
    };
    assert_eq!(convert::console_spec(&spec), words_spec());
}

fn words_spec() -> words::ConsoleSpec {
    words::ConsoleSpec {
        panels: vec![words::Panel {
            id: "topics".into(),
            title: "Topics".into(),
            kind: words::PanelKind::Table,
            columns: vec![words::Column {
                key: "high".into(),
                title: "high".into(),
                role: words::Role::Rate,
            }],
            select: "groups".into(),
        }],
        tabs: vec![words::ConsoleTab {
            id: "t".into(),
            title: "T".into(),
            body: words::Node {
                kind: words::NodeKind::Row,
                id: String::new(),
                title: String::new(),
                weight: 2,
                panel: String::new(),
                from: String::new(),
                children: vec![words::Node {
                    kind: words::NodeKind::Detail,
                    id: "d".into(),
                    title: "query".into(),
                    weight: 0,
                    panel: "topics".into(),
                    from: "table:topics".into(),
                    children: Vec::new(),
                }],
            },
        }],
    }
}

#[test]
fn a_console_row_carries_its_cells_and_tone() {
    let r = wire::ConsoleRow {
        cells: Some(BTreeMap::from([
            ("name".into(), "orders".into()),
            ("_runtime".into(), "1".into()),
        ])),
        tone: "dim".into(),
    };
    let want = words::ConsoleRow {
        cells: HashMap::from([
            ("name".into(), "orders".into()),
            ("_runtime".into(), "1".into()),
        ]),
        tone: "dim".into(),
    };
    assert_eq!(convert::console_row(&r), want);
    assert!(
        convert::console_row(&wire::ConsoleRow::default())
            .cells
            .is_empty(),
        "Go's null cells"
    );
}
