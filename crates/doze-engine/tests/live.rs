//! Instances added to and removed from a running engine (Go's `AddInstance`, `RemoveInstance`): checked as at build
//! time, woken like any other, refused while something needs them, stopped and forgotten on remove.

mod common;

use common::*;
use doze_engine::error::BuildError;
use doze_engine::{Cause, Engine};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_added_service_is_checked_wakes_its_needs_and_is_watched() {
    let d = Dir::new("live-add");
    let e = Engine::builder("t", d.path())
        .service(svc("db", listener(&d, 0)))
        .build()
        .unwrap();
    let mut changes = e.changes();
    changes.borrow_and_update();
    assert_eq!(
        e.add(svc("db", listener(&d, 0))),
        Err(BuildError::Duplicate("db".into()))
    );
    assert_eq!(
        e.add(svc("api", listener(&d, 0)).needs("zz")),
        Err(BuildError::UnknownNeed {
            service: "api".into(),
            need: "zz".into()
        })
    );
    assert!(e.get("api").is_none(), "a refused add leaves nothing");
    e.add(svc("api", listener(&d, 0)).needs("db")).unwrap();
    assert!(changes.has_changed().unwrap(), "watchers see the add");
    assert!(e.stack().service("api").is_some());
    e.wake("api", Cause::Command).await.unwrap();
    assert!(e.get("db").unwrap().state.running(), "its need woke first");
    let pids = [pid_of(&e, "api"), pid_of(&e, "db")];
    e.shutdown().await;
    assert!(all_gone(&pids).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_is_refused_while_needed_then_stops_and_forgets() {
    let d = Dir::new("live-remove");
    let e = Engine::builder("t", d.path())
        .service(svc("db", listener(&d, 0)))
        .service(svc("api", listener(&d, 0)).needs("db"))
        .build()
        .unwrap();
    e.wake("api", Cause::Command).await.unwrap();
    let err = e.remove("db").await.unwrap_err().to_string();
    assert_eq!(err, "instance \"db\" is needed by \"api\"; remove it first");
    assert!(
        e.get("db").unwrap().state.running(),
        "a refused remove stops nothing"
    );
    let api = pid_of(&e, "api");
    e.remove("api").await.unwrap();
    assert!(e.get("api").is_none() && e.stack().service("api").is_none());
    assert!(
        all_gone(&[api]).await,
        "removing a running service stops it"
    );
    assert!(
        e.wake("api", Cause::Command).await.is_err(),
        "a removed name is not declared"
    );
    // What it held is released: db can go now, and a name can come back.
    let db = pid_of(&e, "db");
    e.remove("db").await.unwrap();
    assert!(all_gone(&[db]).await);
    assert!(e.snapshot().is_empty());
    e.add(svc("db", listener(&d, 0))).unwrap();
    let ep = tokio::time::timeout(Duration::from_secs(10), e.wake("db", Cause::Command))
        .await
        .unwrap()
        .unwrap();
    assert!(ep.backend.starts_with("127.0.0.1:"));
    let pid = pid_of(&e, "db");
    e.shutdown().await;
    assert!(all_gone(&[pid]).await);
}
