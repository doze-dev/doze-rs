//! Wake on demand with real processes: one start shared by every caller, needs woken and held first in Go's order,
//! holders, the reaper, pin and off.

mod common;

use common::*;
use doze_engine::events::Kind;
use doze_engine::{Cause, Engine, Holder, State};
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_wakes_of_a_cold_service_share_one_start() {
    let d = Dir::new("coalesce");
    let e = Engine::builder("t", d.path())
        .service(svc("db", listener(&d, 300)))
        .build()
        .unwrap();
    let mut waits = tokio::task::JoinSet::new();
    for _ in 0..10 {
        let e = e.clone();
        waits.spawn(async move { e.wake("db", Cause::Connection).await });
    }
    let mut eps = Vec::new();
    while let Some(r) = waits.join_next().await {
        eps.push(r.unwrap().unwrap());
    }
    assert_eq!(lines(&d.file("starts")), 1, "one start for ten wakes");
    assert!(eps.windows(2).all(|w| w[0] == w[1]) && eps[0].backend.starts_with("127.0.0.1:"));
    assert_eq!(
        e.get("db").unwrap().state,
        State::Idle,
        "running with nothing holding it"
    );
    e.acquire("db");
    assert_eq!(e.get("db").unwrap().state, State::Active);
    let pid = pid_of(&e, "db");
    e.shutdown().await;
    assert!(all_gone(&[pid]).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wake_starts_needs_first_holds_them_and_releases_them_on_stop() {
    let d = Dir::new("needs");
    let e = Engine::builder("shop", d.path())
        .service(svc("web", listener(&d, 0)).needs("api"))
        .service(svc("api", listener(&d, 0)).needs("db").needs("cache"))
        .service(svc("db", listener(&d, 0)))
        .service(svc("cache", listener(&d, 0)))
        .build()
        .unwrap();
    let mut events = e.events();
    let t = Instant::now();
    e.wake("web", Cause::Connection).await.unwrap();
    let took = t.elapsed();
    eprintln!("woke web, api, db and cache in {took:?}");
    // Go: web, api, db and cache (postgres, valkey) in about 0.7 s (parity checklist TMR-10); this shape with this
    // helper: Go 61-85 ms warm, ~165 ms for the first spawn; Rust 51-57 ms warm, ~160 ms first (2026-11-14).
    // Windows runners spawn processes several times slower (1.52 s seen on windows-latest): the budget is doubled there.
    let budget = if std::env::consts::OS == "windows" {
        3000
    } else {
        1500
    };
    assert!(
        took < Duration::from_millis(budget),
        "woke four in {took:?}"
    );
    let mut wakes = Vec::new();
    let mut readies = Vec::new();
    while let Ok(ev) = events.try_recv() {
        match ev.kind {
            Kind::Wake => wakes.push((ev.instance, ev.cause)),
            Kind::Ready => readies.push(ev.instance),
            _ => {}
        }
    }
    let woke: Vec<_> = wakes.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(woke, ["web", "api", "db", "cache"]);
    assert_eq!(wakes[1].1, Cause::Dependency("web".into()));
    assert_eq!(readies, ["db", "cache", "api", "web"]);
    assert_eq!(e.holders("api"), [Holder::Dependant("web".into())]);
    assert_eq!(e.holders("db"), [Holder::Dependant("api".into())]);
    assert!(e.holders("web").is_empty());
    let pids: Vec<u32> = ["web", "api", "db", "cache"]
        .iter()
        .map(|n| pid_of(&e, n))
        .collect();
    e.stop("web", Cause::Command).await.unwrap();
    assert_eq!(e.get("web").unwrap().state, State::Reaped);
    assert_eq!(
        e.get("api").unwrap().state,
        State::Idle,
        "released: nothing holds it now"
    );
    e.shutdown().await;
    assert!(all_gone(&pids).await);
    // Warm (the binary was started before, as Go's 60-85 ms for this shape was measured after `up`).
    let warm = Engine::builder("shop", d.path())
        .service(svc("web", listener(&d, 0)).needs("api"))
        .service(svc("api", listener(&d, 0)).needs("db").needs("cache"))
        .service(svc("db", listener(&d, 0)))
        .service(svc("cache", listener(&d, 0)))
        .build()
        .unwrap();
    let t = Instant::now();
    warm.wake("web", Cause::Connection).await.unwrap();
    eprintln!("woke them again in {:?}", t.elapsed());
    let again: Vec<u32> = ["web", "api", "db", "cache"]
        .iter()
        .map(|n| pid_of(&warm, n))
        .collect();
    warm.shutdown().await;
    assert!(all_gone(&again).await);
    for n in ["web", "api", "db", "cache"] {
        assert_eq!(e.get(n).unwrap().state, State::Reaped);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_reaper_puts_an_idle_service_to_sleep_but_not_a_pinned_or_held_one() {
    let d = Dir::new("reap");
    let e = Engine::builder("t", d.path())
        .idle_timeout(Duration::from_millis(600))
        .service(svc("a", listener(&d, 0)))
        .service(svc("b", listener(&d, 0)))
        .service(svc("c", listener(&d, 0)))
        .build()
        .unwrap();
    let _bg = e.background();
    for n in ["a", "b", "c"] {
        e.wake(n, Cause::Command).await.unwrap();
    }
    let pids: Vec<u32> = ["a", "b", "c"].iter().map(|n| pid_of(&e, n)).collect();
    assert!(e.toggle_pin("b").unwrap());
    e.acquire("c");
    let slept = eventually(Duration::from_secs(4), || {
        e.get("a").is_some_and(|i| i.state == State::Reaped)
    })
    .await;
    assert!(slept, "a sleeps after its idle time");
    assert!(all_gone(&pids[..1]).await);
    assert!(e.get("b").unwrap().state.running(), "pinned");
    assert!(e.get("c").unwrap().state.running(), "held");
    assert_eq!(e.holders("b"), [Holder::Pin]);
    assert_eq!(e.holders("c"), [Holder::Connections(1)]);
    e.release("c");
    assert!(
        eventually(Duration::from_secs(4), || e
            .get("c")
            .is_some_and(|i| i.state == State::Reaped))
        .await
    );
    e.shutdown().await;
    assert!(all_gone(&pids).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn off_refuses_every_start_until_on() {
    let d = Dir::new("off");
    let e = Engine::builder("t", d.path())
        .service(svc("db", listener(&d, 0)))
        .build()
        .unwrap();
    e.wake("db", Cause::Command).await.unwrap();
    let pid = pid_of(&e, "db");
    e.set_off("db", true).await.unwrap();
    assert!(all_gone(&[pid]).await);
    let err = e.wake("db", Cause::Connection).await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "db is off: `doze on db` puts it back into this run"
    );
    e.set_off("db", false).await.unwrap();
    assert_eq!(
        e.get("db").unwrap().state,
        State::Reaped,
        "on starts nothing by itself"
    );
    e.wake("db", Cause::Command).await.unwrap();
    let missing = e.wake("nope", Cause::Command).await.unwrap_err();
    assert!(missing.not_found());
    assert_eq!(
        missing.to_string(),
        "instance \"nope\" is not declared in doze.hcl"
    );
    let pid = pid_of(&e, "db");
    e.shutdown().await;
    assert!(all_gone(&[pid]).await);
    let late = e.wake("db", Cause::Command).await.unwrap_err();
    assert_eq!(
        late.to_string(),
        "daemon is shutting down; not booting \"db\""
    );
}

/// The engine probes a port and the process binds it a moment later: something else (another doze daemon, any app
/// asking for that port by number) can take it in between, and the process dies with "address in use" through no fault
/// of its own (CI: logs_and_journal on macOS, os error 48; sync on Windows, os error 10048). The start is made again on
/// another port, and the wake succeeds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_port_taken_before_the_process_binds_it_is_chosen_again() {
    let d = Dir::new("lostport");
    let given = d.file("port");
    let spec = listener(&d, 400)
        .env("PORT_FILE", &given.to_string_lossy())
        .ready(doze_engine::Probe::Log("listening on".into()));
    let e = Engine::builder("t", d.path())
        .service(svc("web", spec))
        .build()
        .unwrap();
    let wake = {
        let e = e.clone();
        tokio::spawn(async move { e.wake("web", Cause::Command).await })
    };
    let read = || {
        std::fs::read_to_string(&given)
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
    };
    assert!(eventually(Duration::from_secs(5), || read().is_some()).await);
    let first = read().unwrap();
    let thief = std::net::TcpListener::bind(("127.0.0.1", first)).unwrap();
    let ep = wake.await.unwrap().unwrap();
    assert_eq!(lines(&d.file("starts")), 2, "started again once");
    assert_eq!(
        e.logs().runs("web").unwrap().len(),
        2,
        "the failed start's run is kept"
    );
    let second = read().unwrap();
    assert_ne!(second, first);
    assert_eq!(ep.backend, format!("127.0.0.1:{second}"));
    drop(thief);
    let pid = pid_of(&e, "web");
    e.shutdown().await;
    assert!(all_gone(&[pid]).await);
}

/// A process that dies while starting for a reason of its own (its port is free) is not started again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_process_that_dies_starting_with_its_port_free_is_not_started_again() {
    let d = Dir::new("ownfault");
    let spec = doze_engine::ProcessSpec::new(run(&["exit", "3", "200"]), d.path())
        .port(1)
        .ready(doze_engine::Probe::Tcp);
    let e = Engine::builder("t", d.path())
        .service(svc("web", spec))
        .build()
        .unwrap();
    let err = e.wake("web", Cause::Command).await.unwrap_err().to_string();
    assert!(err.contains("exited during startup"), "{err}");
    assert_eq!(e.logs().runs("web").unwrap().len(), 1, "one start, one run");
    e.shutdown().await;
}
