//! The hub's part of the Go app's view goldens (`internal/desk/view/testdata`, doze 23b73d6, copied to
//! `testdata/go/view`). Go's view tests run each fixture hub through a real hub and render pages from it; here the
//! same fixtures run through this hub over a real socket, and every fact a golden shows that comes from the hub's
//! state (rather than from the view's layout) is checked against what the hub folded: the instances and their
//! states, the meters and the series, the log lines, the journal, the console tables.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::too_many_arguments
)]

mod check;
#[path = "../common/mod.rs"]
mod common;
mod pages;

use check::{check_rows, check_series, check_table, g, golden, s};
use common::{following, until};
use doze_control::Topic;
use doze_control::fixture::FixtureDaemon;
use doze_desk::{Hub, StackKey, StackView, Watch, convert};
use doze_display as words;
use std::collections::BTreeSet;

type Followed = (
    common::Home,
    FixtureDaemon,
    Hub,
    StackKey,
    (Watch, Watch),
    StackView,
);

/// The shop fixture through the hub as Go's view tests run it: meters and logs watched, all of them in.
fn shop() -> Option<Followed> {
    let (home, d, hub, key) = following("shop")?;
    let w = (hub.watch(&key, Topic::Stats), hub.watch(&key, Topic::Logs));
    until(&hub, &key, "the meters and the logs", |s| {
        s.series.get("api").is_some_and(|x| x.len() == 240) && s.logs_added >= 23
    });
    let st = until(&hub, &key, "the journal", |s| s.events.len() == 11);
    Some((home, d, hub, key, w, st))
}

#[test]
fn shop_home_and_instances() {
    let Some((_home, _d, _hub, _key, _w, st)) = shop() else {
        return;
    };
    let home = golden("shop-home");
    assert_eq!(st.name, s(g(&home, "/stack")));
    assert_eq!(st.dir, s(g(&home, "/dir")));
    assert!(g(&home, "/banner/ok").as_bool().unwrap());
    assert_eq!(
        words::banner(
            false,
            !st.link.is_live(),
            Default::default(),
            Default::default()
        ),
        None
    );
    check_rows(&st, &home);
    let total: i64 = st.instances.iter().map(|v| v.ram).sum();
    assert_eq!(words::mb(total), s(g(&home, "/vitals/mem/total")));

    check_series(&st, &golden("shop-api"));
    check_series(&st, &golden("shop-db"));
}

#[test]
fn shop_logs() {
    let Some((_home, _d, hub, key, _w, st)) = shop() else {
        return;
    };
    // The api logs page: its ten lines, the last five shown.
    let page = golden("shop-api-logs");
    let api = hub.logs(&key, "api");
    assert_eq!(g(&page, "/total").as_u64(), Some(api.len() as u64));
    let shown: Vec<&str> = g(&page, "/lines")
        .as_array()
        .unwrap()
        .iter()
        .map(|l| s(g(l, "/line")))
        .collect();
    assert_eq!(
        g(&page, "/from").as_u64(),
        Some(0),
        "the window starts at the oldest line"
    );
    assert_eq!(shown.len(), 5);
    for (want, got) in shown.iter().zip(&api) {
        // The page drops a level word it shows as a badge.
        assert!(got.line.ends_with(want), "{want:?} is not {:?}", got.line);
    }

    // Every stack's lines and the journal merged: the counts are of both.
    let page = golden("shop-all-logs-warn");
    let all = hub.logs(&key, "");
    let counted: u64 = g(&page, "/counts")
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_u64().unwrap())
        .sum();
    assert_eq!(counted, (all.len() + st.events.len()) as u64);
    let warn = g(&page, "/lines").as_array().unwrap().first().unwrap();
    assert!(
        all.iter()
            .any(|l| l.instance == s(g(warn, "/instance"))
                && l.line.ends_with(s(g(warn, "/line"))))
    );
    let named: BTreeSet<&str> = all
        .iter()
        .map(|l| l.instance.as_str())
        .chain(st.events.iter().map(|e| e.instance.as_str()))
        .collect();
    for inst in g(&page, "/instances").as_array().unwrap().iter().map(s) {
        assert!(inst == "doze" || named.contains(inst), "{inst}");
    }
}

#[test]
fn crash() {
    let Some((_home, _d, hub, key)) = following("crash") else {
        return;
    };
    let _w = hub.watch(&key, Topic::Stats);
    let st = until(&hub, &key, "the meters", |s| {
        s.series.get("db").is_some_and(|x| x.len() == 240)
    });
    check_rows(&st, &golden("crash-home"));
    let page = golden("crash-api");
    let api = st.instance("api").unwrap();
    assert_eq!(
        words::look(&convert::instance(api)).word(),
        s(g(&page, "/look/word"))
    );
    assert!(api.last_error.starts_with(s(g(&page, "/crash/error"))));
    assert_eq!(
        format!("try {} of {}", api.restart_count, api.restart_max),
        s(g(&page, "/crash/tries"))
    );
}

#[test]
fn modules() {
    let Some((_home, _d, hub, key)) = following("modules") else {
        return;
    };
    let _w = (
        hub.watch_console(&key, "db"),
        hub.watch_console(&key, "events"),
    );
    let st = until(&hub, &key, "the consoles", |s| {
        let rows = |i: &str, p: &str| {
            s.consoles
                .get(i)
                .and_then(|c| c.tables.get(p))
                .map_or(0, Vec::len)
        };
        rows("db", "sessions") == 4
            && rows("events", "topics") == 3
            && rows("events", "groups") == 2
    });
    let db = golden("modules-db-activity");
    check_table(&st, "db", g(&db, "/body/children/1/children/0"));
    let events = golden("modules-events-console");
    for block in g(&events, "/body/children/0/children").as_array().unwrap() {
        check_table(&st, "events", block);
    }
    assert_eq!(st.consoles.get("events").unwrap().state, "active");
}

#[test]
fn larder() {
    let Some((_home, _d, hub, key)) = following("larder") else {
        return;
    };
    let st = hub.stack(&key).unwrap();
    let map = golden("larder-map");
    for n in g(&map, "/nodes").as_array().unwrap() {
        assert!(st.instance(s(g(n, "/name"))).is_some(), "{}", g(n, "/name"));
    }
    for e in g(&map, "/edges").as_array().unwrap() {
        let from = st.instance(s(g(e, "/from"))).unwrap();
        assert!(
            from.needs.iter().any(|n| n == s(g(e, "/to"))),
            "{} needs {}",
            g(e, "/from"),
            g(e, "/to")
        );
    }
    for r in g(&golden("larder-grid"), "/rows").as_array().unwrap() {
        assert!(st.instance(s(g(r, "/name"))).is_some(), "{}", g(r, "/name"));
    }
}

#[test]
fn empty() {
    let Some((_home, _d, hub, key)) = following("empty") else {
        return;
    };
    let st = hub.stack(&key).unwrap();
    assert!(st.instances.is_empty() && st.running && st.link.is_live());
    assert_eq!(
        st.name, "p-1",
        "a daemon that says no name is named by its folder"
    );
}
