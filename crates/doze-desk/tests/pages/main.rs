//! The pages against the Go app's view JSON, field for field: the overview, the stopped overview, the Grid and the
//! Library. Two sets of Go JSON:
//!
//! - `testdata/go/view`: Go's own view goldens (`internal/desk/view/testdata`, doze 23b73d6): the fixture hubs run
//!   through a hub, rendered at Go's epoch.
//! - `testdata/go/serve`: what Go's app served for the same fixtures (`doze open --serve --fixture shop|larder|crash`
//!   on a stopped clock, `DOZE_FIXTURE_NOW=2026-10-11T14:04:40Z`, captured with `POST /api/home` and `/api/library`):
//!   the Library, and a stopped stack's page, which have no view golden. Paths under the fixture's folder read
//!   `$WORK`; `map` and `side` (another step's) were dropped when they were captured.
//!
//! Go writes `null` for an empty list and leaves empty fields out; both sides are read with those as the same.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

#[path = "../common/mod.rs"]
mod common;

use common::{Home, epoch, following, until};
use doze_control::Topic;
use doze_desk::library::{Hints, library};
use doze_desk::overview::{Shape, home, refs_in, scan, stopped_home};
use doze_desk::{Hub, StackView};
use serde_json::{Map, Value};
use std::path::Path;
use std::time::Duration;

fn testdata(rel: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join(rel),
    )
    .unwrap()
}

fn json(rel: &str, work: &str) -> Value {
    // The folder goes in as a JSON string would hold it (a Windows path's backslashes escaped).
    let work = serde_json::to_string(work).unwrap();
    let work = work.trim_matches('"');
    serde_json::from_str(&testdata(rel).replace("$WORK", work)).unwrap()
}

/// Every string with its path separators written as `/`, so a folder compares the same on Windows.
fn slashes(v: Value) -> Value {
    match v {
        Value::String(s) => Value::String(s.replace('\\', "/")),
        Value::Array(a) => Value::Array(a.into_iter().map(slashes).collect()),
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, v)| (k, slashes(v))).collect()),
        other => other,
    }
}

/// `v` with the empty left out, as Go's `omitempty` and `null` lists leave them, and `drop`'s keys gone.
fn norm(v: &Value, drop: &[&str]) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| !drop.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), norm(v, drop)))
                .filter(|(_, v)| !empty(v))
                .collect::<Map<_, _>>(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(|v| norm(v, drop)).collect()),
        other => other.clone(),
    }
}

fn empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Array(a) => a.is_empty(),
        Value::String(s) => s.is_empty(),
        Value::Bool(b) => !b,
        Value::Object(m) => m.is_empty(),
        Value::Number(_) => false,
    }
}

/// Fails with the first path where `got` and `want` differ.
fn same(got: &Value, want: &Value, at: &str) {
    match (got, want) {
        (Value::Object(g), Value::Object(w)) => {
            let keys: std::collections::BTreeSet<&String> = g.keys().chain(w.keys()).collect();
            for k in keys {
                same(
                    g.get(k).unwrap_or(&Value::Null),
                    w.get(k).unwrap_or(&Value::Null),
                    &format!("{at}/{k}"),
                );
            }
        }
        (Value::Array(g), Value::Array(w)) => {
            assert_eq!(
                g.len(),
                w.len(),
                "{at}: {} items, Go has {}\n got {got}\nwant {want}",
                g.len(),
                w.len()
            );
            for (i, (g, w)) in g.iter().zip(w).enumerate() {
                same(g, w, &format!("{at}/{i}"));
            }
        }
        // Improvement, on purpose: Go's view snapshot left the stack's idle time out (`view.snapshot`), so a dozing
        // row said "quiet; sleeps soon"; the hub keeps it, so the row counts down, as the instance page did.
        (Value::String(g), Value::String(w))
            if at.ends_with("/because")
                && w == "quiet; sleeps soon"
                && g.starts_with("sleeps in ") => {}
        (Value::Number(g), Value::Number(w)) => {
            let (g, w) = (g.as_f64().unwrap(), w.as_f64().unwrap());
            assert!((g - w).abs() < 1e-9, "{at}: {g} is not Go's {w}");
        }
        _ => assert_eq!(got, want, "{at}"),
    }
}

/// Compares a page with Go's JSON, without the parts named in `drop`.
fn check<T: serde::Serialize>(page: &T, go: &Value, drop: &[&str]) {
    let got = slashes(norm(&serde_json::to_value(page).unwrap(), drop));
    same(&got, &slashes(norm(go, drop)), "");
}

/// A fixture hub through the hub, its meters and logs watched, as Go's view tests run it.
fn watched(
    name: &str,
) -> Option<(
    common::Home,
    doze_control::fixture::FixtureDaemon,
    Hub,
    StackView,
)> {
    let (home, d, hub, key) = following(name)?;
    let _w = (hub.watch(&key, Topic::Stats), hub.watch(&key, Topic::Logs));
    let st = until(&hub, &key, "the meters", |s| {
        s.series.values().any(|x| x.len() == 240)
    });
    Some((home, d, hub, st))
}

#[test]
fn shop_home_is_gos() {
    let Some((_h, _d, _hub, st)) = watched("shop") else {
        return;
    };
    let h = home(&st, epoch());
    // The key is the temp home's path; `up` counts from when the hub first saw the daemon.
    check(
        &h,
        &json("go/view/shop-home.json", ""),
        &["key", "map", "side", "view", "up"],
    );
    assert_eq!((h.awake, h.total, h.groups.len()), (5, 10, 4));
}

#[test]
fn crash_home_is_gos() {
    let Some((_h, _d, _hub, st)) = watched("crash") else {
        return;
    };
    check(
        &home(&st, epoch()),
        &json("go/view/crash-home.json", ""),
        &["key", "map", "side", "view", "up"],
    );
}

#[test]
fn larder_grid_is_gos() {
    let Some((_h, _d, hub, key)) = following("larder") else {
        return;
    };
    let st = hub.stack(&key).unwrap();
    let mut g = home(&st, epoch()).grid;
    g.refer(&scan(&testdata("go/hcl/larder.hcl")).refs);
    check(&g, &json("go/view/larder-grid.json", ""), &[]);
    // Go's own assertions on the board.
    let rows: Vec<&str> = g.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        rows,
        [
            "storefront",
            "gateway",
            "identity",
            "fulfilment",
            "recommendations",
            "orders",
            "catalog",
            "payments",
            "notifications"
        ]
    );
    assert_eq!(
        g.foot,
        ["1", "1", "1", "1", "2", "3", "6", "5", "3", "2", "1", ""]
    );
}

#[test]
fn larder_home_as_go_served_it() {
    let Some((_h, _d, hub, key)) = following("larder") else {
        return;
    };
    let st = until(&hub, &key, "described", |s| !s.dir.is_empty());
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("doze.hcl"), testdata("go/hcl/larder.hcl")).unwrap();
    let mut h = home(&st, epoch());
    h.grid.refer(&refs_in(dir.path()).unwrap());
    // The served page's figures are the meters' last second; without stats watched, the strip is the snapshot's.
    check(
        &h.grid,
        &json("go/serve/larder-home-larder.json", "")["grid"],
        &[],
    );
    let groups = norm(
        &serde_json::to_value(&h.groups).unwrap(),
        &["up", "mem", "cpu", "conns"],
    );
    let want = norm(
        &json("go/serve/larder-home-larder.json", "")["groups"],
        &["up", "mem", "cpu", "conns"],
    );
    same(&groups, &want, "/groups");
}

/// A home with the stopped stack every Go fixture home carries: acme, up three days ago with api, web and db.
fn acme(home: &Home) -> (tempfile::TempDir, String) {
    let work = tempfile::tempdir().unwrap();
    let dir = work.path().join("code/acme");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("doze.hcl"), testdata("go/hcl/acme.hcl")).unwrap();
    let then = epoch() - Duration::from_secs(3 * 24 * 3600);
    let at = |t: std::time::SystemTime| {
        doze_control::Timestamp::from_system_time(t)
            .as_str()
            .to_owned()
    };
    home.record(
        "acme",
        &serde_json::json!({
            "schema": 1,
            "config": dir.join("doze.hcl").display().to_string(),
            "name": "acme",
            "last_up": at(then),
            "last_down": at(then + Duration::from_secs(6 * 3600)),
            "last_set": ["api", "web", "db"],
        })
        .to_string(),
    );
    let w = work.path().display().to_string();
    (work, w)
}

#[test]
fn a_stopped_stack_as_go_served_it() {
    let home = Home::new();
    let (_work, w) = acme(&home);
    let hub = Hub::new(home.options());
    hub.refresh();
    let st = hub.stacks().into_iter().find(|s| s.name == "acme").unwrap();
    let shape = Shape::read(Path::new(&st.dir), Some(&testdata("cli/acme-status.json")));
    let h = stopped_home(&st, &shape, epoch());
    check(
        &h,
        &json("go/serve/shop-home-acme.json", &w),
        &["key", "view", "vitals", "grid"],
    );
}

#[test]
fn the_library_as_go_served_it() {
    let Some((home, _d, hub, _key)) = following("shop") else {
        return;
    };
    let (_work, w) = acme(&home);
    hub.refresh();
    let stacks = until_two(&hub);
    let shapes = |st: &StackView| {
        (!st.running)
            .then(|| Shape::read(Path::new(&st.dir), Some(&testdata("cli/acme-status.json"))))
    };
    let lib = library(&stacks, &shapes, Hints::default(), epoch());
    // The https pill's readiness and the machine line are the service's (they ask the machine); `when` of a running
    // stack counts from when this hub first saw it.
    let mut go = json("go/serve/shop-library.json", &w);
    for s in go["stacks"].as_array_mut().unwrap() {
        if let Some(a) = s["addresses"].as_array_mut() {
            a.retain(|a| !a["text"].as_str().unwrap_or("").starts_with("https://"));
        }
    }
    let mut got = serde_json::to_value(&lib).unwrap();
    for s in got["stacks"].as_array_mut().unwrap() {
        if let Some(a) = s["addresses"].as_array_mut() {
            a.retain(|a| !a["text"].as_str().unwrap_or("").starts_with("https://"));
        }
    }
    same(
        &norm(&got, &["key", "machine", "when", "more"]),
        &norm(&go, &["key", "machine", "when", "more"]),
        "",
    );
    assert_eq!(lib.stacks[1].when, "up 3d ago");
}

fn until_two(hub: &Hub) -> Vec<StackView> {
    common::wait("both stacks", || hub.stacks().len() == 2);
    hub.stacks()
}

#[test]
fn the_list_pane_says_what_the_instance_page_said() {
    let Some((_h, _d, _hub, st)) = watched("shop") else {
        return;
    };
    for (name, golden) in [
        ("api", "go/view/shop-api.json"),
        ("db", "go/view/shop-db.json"),
    ] {
        let mut p = doze_desk::overview::pane(&st, name, epoch()).unwrap();
        // The Go view's rows; the pane, as the instance page, adds the process's IP and port as Go's service did on
        // top of them.
        let page = doze_desk::page::instance::instance_page(
            &st,
            name,
            None,
            epoch(),
            doze_desk::page::time::Zone::UTC,
        );
        assert_eq!(
            p.connect, page.connect,
            "{name}: the pane's rows are the page's"
        );
        assert!(
            p.connect.iter().any(|r| r.k == "IP and port"),
            "{name}: the pane has Go's IP and port row"
        );
        p.connect.retain(|r| r.k != "IP and port");
        let go = json(golden, "");
        for part in [
            "connect", "holders", "sleeps", "needs", "because", "up", "pid", "run", "look",
        ] {
            let wrap = |v: &Value| norm(&serde_json::json!({ "v": v }), &[]);
            same(
                &wrap(&serde_json::to_value(&p).unwrap()[part]),
                &wrap(&go[part]),
                &format!("{name}/{part}"),
            );
        }
    }
}
