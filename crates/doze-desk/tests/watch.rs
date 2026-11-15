//! What streams, and when: the Go hub's watch tests (`rings_test.go`), the resubscription when a watch count crosses
//! zero, and module consoles.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Home, daemon, following, script, until, wait};
use doze_control::fixture::FixtureDaemon;
use doze_control::{Op, Request, Topic};
use doze_desk::{Conn, ConsoleWatch, Hub, SAMPLES_KEPT, StackKey};
use doze_display::ConsoleRow;
use std::time::Duration;

/// The subscribe requests the daemon has had, in order.
fn subscribes(d: &FixtureDaemon) -> Vec<Request> {
    d.requests()
        .into_iter()
        .filter(|r| {
            r.known_op() == Some(Op::Subscribe) && !r.topics.contains(&"console".to_owned())
        })
        .collect()
}

fn topics(r: &Request) -> Vec<&str> {
    r.topics.iter().map(String::as_str).collect()
}

#[test]
fn stats_and_logs_stream_only_while_watched() {
    let Some((_home, d, hub, key)) = following("shop") else {
        return;
    };
    let s = hub.stack(&key).unwrap();
    assert!(
        s.series.is_empty() && hub.logs(&key, "api").is_empty(),
        "nothing is watched"
    );
    assert!(!s.metered);

    let stats = hub.watch(&key, Topic::Stats);
    let s = until(&hub, &key, "api's meters", |s| {
        s.series.get("api").is_some_and(|x| x.len() == SAMPLES_KEPT)
    });
    assert!(s.metered);
    let api = s.series.get("api").unwrap();
    assert_eq!(
        (api.cpu.len(), api.conns.len()),
        (SAMPLES_KEPT, SAMPLES_KEPT)
    );
    assert!(
        !s.series.contains_key("mongo"),
        "mongo reports no meters; it has no series"
    );
    // The instance rows carry the latest reading too.
    let row = s.instance("api").unwrap();
    assert_eq!(row.ram as f64, *api.ram.last().unwrap());

    let logs = hub.watch(&key, Topic::Logs);
    wait("every line", || hub.logs(&key, "").len() == 23);
    assert_eq!(hub.logs(&key, "api").len(), 10);
    assert_eq!(hub.logs_version(&key, "api"), 10);

    // A reconnect replays the backlog and the meters; nothing is doubled.
    d.hang_up();
    until(&hub, &key, "live again", |s| s.link.conn == Conn::Live);
    wait("the backlog replayed", || d.held() == 1);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(hub.logs(&key, "api").len(), 10, "after a reconnect");
    assert_eq!(hub.logs(&key, "").len(), 23);
    let s = hub.stack(&key).unwrap();
    assert_eq!(s.series.get("api").unwrap().len(), SAMPLES_KEPT);
    assert_eq!(s.events.len(), 11, "the journal is not doubled");
    drop((stats, logs));
}

#[test]
fn a_watch_outlives_the_stack_it_waits_for() {
    // A page watching a stack that is not running yet, or whose daemon restarts, gets its stats once it is there.
    let Some(d) = daemon("waits", script("shop")) else {
        return;
    };
    let home = Home::new();
    let key = home.stack("shop-1", &d);
    home.remove_socket(&key);
    let hub = Hub::start(home.options());
    let _stats = hub.watch(&key, Topic::Stats);
    std::thread::sleep(Duration::from_millis(50));
    assert!(hub.stacks().is_empty());
    home.put_socket(&key);
    until(&hub, &key, "the meters", |s| {
        s.series.get("api").is_some_and(|x| x.len() == SAMPLES_KEPT)
    });
    assert_eq!(
        hub.logs_version(&key, "api"),
        0,
        "logs were not watched, yet lines were counted"
    );
}

#[test]
fn the_subscription_is_remade_on_the_same_connection_when_a_count_crosses_zero() {
    let Some((_home, d, hub, key)) = following("shop") else {
        return;
    };
    wait("subscribed", || subscribes(&d).len() == 1);
    let first = subscribes(&d);
    assert_eq!(topics(first.first().unwrap()), ["instances", "journal"]);
    let conns = d.accepted();

    let a = hub.watch(&key, Topic::Stats);
    wait("resubscribed with stats", || subscribes(&d).len() == 2);
    let b = hub.watch(&key, Topic::Stats);
    let l = hub.watch(&key, Topic::Logs);
    wait("resubscribed with logs", || subscribes(&d).len() == 3);
    let last = subscribes(&d).pop().unwrap();
    assert_eq!(topics(&last), ["instances", "journal", "stats", "logs"]);
    assert_eq!(last.backlog, doze_desk::BACKLOG);
    assert_eq!(hub.watchers(&key, Topic::Stats), 2);

    drop(a);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        subscribes(&d).len(),
        3,
        "a second page leaving changes nothing"
    );
    drop(b);
    wait("resubscribed without stats", || subscribes(&d).len() == 4);
    assert_eq!(
        topics(&subscribes(&d).pop().unwrap()),
        ["instances", "journal", "logs"]
    );
    assert!(!hub.stack(&key).unwrap().metered);
    drop(l);
    wait("back to the start", || subscribes(&d).len() == 5);
    assert_eq!(
        topics(&subscribes(&d).pop().unwrap()),
        ["instances", "journal"]
    );

    assert_eq!(d.accepted(), conns, "every change rode the one connection");
    // The Go fixture's events are unnumbered, so each resubscribe asks from the start and the replay is folded
    // away: the journal is not doubled.
    assert!(
        subscribes(&d)
            .iter()
            .all(|r| r.since == doze_control::JOURNAL_FROM_START)
    );
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(hub.stack(&key).unwrap().events.len(), 11);
}

/// The tail stream of the events console, once it has rows.
fn tail(
    hub: &Hub,
    key: &StackKey,
    what: &str,
    ok: impl Fn(&[ConsoleRow]) -> bool,
) -> Vec<ConsoleRow> {
    let s = until(hub, key, what, |s| {
        s.consoles
            .get("events")
            .and_then(|c| c.streams.get("tail"))
            .is_some_and(|r| !r.is_empty() && ok(r))
    });
    s.consoles
        .get("events")
        .unwrap()
        .streams
        .get("tail")
        .unwrap()
        .clone()
}

/// The modules fixture with db's and events' consoles shown and their tables in.
fn consoles() -> Option<(
    Home,
    FixtureDaemon,
    Hub,
    StackKey,
    ConsoleWatch,
    ConsoleWatch,
)> {
    let (home, d, hub, key) = following("modules")?;
    let db = hub.watch_console(&key, "db");
    let events = hub.watch_console(&key, "events");
    let s = until(&hub, &key, "both consoles", |s| {
        let rows = |i: &str, p: &str| {
            s.consoles
                .get(i)
                .and_then(|c| c.tables.get(p))
                .map_or(0, Vec::len)
        };
        rows("db", "sessions") == 4
            && rows("events", "topics") == 3
            && s.consoles
                .get("events")
                .is_some_and(|c| c.spec.is_some() && c.words.is_some())
    });
    let ev = s.consoles.get("events").unwrap();
    assert!(ev.streams.is_empty(), "no stream until one is followed");
    assert!(!ev.actions.is_empty());
    Some((home, d, hub, key, db, events))
}

#[test]
fn a_console_follows_the_stream_and_row_asked_for() {
    let Some((_home, _d, hub, key, _db, _events)) = consoles() else {
        return;
    };
    hub.follow_console(&key, "events", "tail", "orders");
    let rows = tail(&hub, &key, "the tail of orders", |_| true);
    assert!(
        rows.iter()
            .all(|r| ["", "orders"].contains(&r.cell("_for"))),
        "{rows:?}"
    );

    hub.follow_console(&key, "events", "tail", "emails");
    let rows = tail(&hub, &key, "the tail of emails", |r| {
        r.iter().all(|r| r.cell("_for") != "orders")
    });
    assert!(rows.iter().any(|r| r.cell("_for") == "emails"), "{rows:?}");
}

#[test]
fn a_console_streams_while_a_page_shows_it() {
    let Some((_home, d, hub, key, db, events)) = consoles() else {
        return;
    };
    let asked = |d: &FixtureDaemon| {
        d.requests()
            .into_iter()
            .filter(|r| r.topics.contains(&"console".to_owned()))
            .count()
    };
    let before = asked(&d);
    drop(events);
    until(&hub, &key, "events' console forgotten", |s| {
        !s.consoles.contains_key("events")
    });
    let other = hub.watch_console(&key, "db");
    drop(db);
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        hub.stack(&key).unwrap().consoles.contains_key("db"),
        "another page still shows db"
    );
    assert_eq!(
        asked(&d),
        before,
        "a second page on a shown console asks nothing new"
    );
    drop(other);
    until(&hub, &key, "db's console forgotten", |s| {
        s.consoles.is_empty()
    });
    wait("both console streams ended", || d.streams() == 1);
}
