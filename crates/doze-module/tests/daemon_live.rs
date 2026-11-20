//! `add` and `remove` over the wire with the echo module (Go's `AddInstance`, `RemoveInstance`): a block read alone
//! becomes an instance with its module's labels and its front, a connection wakes it, a refused block says why, and a
//! remove stops it, closes its front and (with `wipe`) takes its data.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod daemon_support;

use daemon_support::*;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// One echo line through `addr`.
async fn echoed(addr: &str) -> bool {
    let Ok(conn) = tokio::net::TcpStream::connect(addr).await else {
        return false;
    };
    let (r, mut w) = conn.into_split();
    w.write_all(b"ping\n").await.unwrap();
    let mut lines = BufReader::new(r).lines();
    while let Ok(Ok(Some(line))) =
        tokio::time::timeout(Duration::from_secs(20), lines.next_line()).await
    {
        if line.contains("ping") {
            return true;
        }
    }
    false
}

async fn ask(sock: &std::path::Path, line: String) -> serde_json::Value {
    let sock = sock.to_path_buf();
    tokio::task::spawn_blocking(move || Raw::open(&sock).ask(&line))
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_added_block_is_fronted_woken_and_removed_with_its_data() {
    let (dir, _port) = project("live");
    let Some((d, s, sock, front)) = serve(&dir).await else {
        return;
    };
    let port = free_port();
    let block = format!(
        "module echo \"late\" {{\n  version = \"2\"\n  port = {port}\n  settings = {{\n    ready = \"log\"\n  }}\n}}\n"
    );
    let add = serde_json::json!({"id": 1, "op": "add", "block": block}).to_string();
    let r = ask(&sock, add.clone()).await;
    assert_eq!(r["ok"], true, "{r}");
    let v = &r["instances"][0];
    let addr = format!("127.0.0.1:{port}");
    assert_eq!(
        (
            v["name"].as_str(),
            v["state"].as_str(),
            v["endpoint"].as_str()
        ),
        (Some("late"), Some("reaped"), Some(addr.as_str())),
        "{v}"
    );
    assert_eq!(
        v["url"],
        format!("echo://{addr}/late"),
        "published by its module"
    );
    let r = ask(&sock, add).await;
    assert_eq!(
        r["error"], "an instance named \"late\" already exists",
        "{r}"
    );

    assert!(
        echoed(&addr).await,
        "a connection wakes it through its front"
    );
    let data = d.engine().stack().cluster_dir("late");
    assert!(data.exists() && d.engine().get("late").unwrap().state.running());
    // Ready by a log line: where it is reached is the grant alone, its loopback port chosen at this start.
    let backend = d.engine().get("late").unwrap().backend;
    assert!(
        backend.starts_with("unix:")
            || (backend.starts_with("127.0.0.1:") && !backend.ends_with(":0")),
        "{backend}"
    );

    let r = ask(
        &sock,
        r#"{"id":2,"op":"remove","db":"late","wipe":true}"#.into(),
    )
    .await;
    assert_eq!(r["ok"], true, "{r}");
    assert!(d.engine().get("late").is_none() && !data.exists());
    assert!(
        tokio::net::TcpStream::connect(&addr).await.is_err(),
        "its front is closed"
    );
    gone_from_the_wire(&sock).await;
    s.stop();
    front.close();
    d.engine().shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_block_that_cannot_be_added_says_why_and_leaves_nothing() {
    let (dir, _port) = project("live-refused");
    let Some((d, s, sock, front)) = serve(&dir).await else {
        return;
    };
    let cases = [
        (
            "module echo \"r\" {\n  version = \"2\"\n  port = 1\n  greeting = module.e.url\n}\n",
            "a live-added service can't reference other services yet",
        ),
        (
            "module echo \"a\" {\n  version = \"2\"\n}\nmodule echo \"b\" {\n  version = \"2\"\n}\n",
            "expected exactly one module or app block, got 2",
        ),
        (
            "module echo \"v\" {\n  version = \"7\"\n}\n",
            "is not a version the echo module serves",
        ),
        ("", "add needs a block"),
    ];
    for (block, want) in cases {
        let line = serde_json::json!({"id": 1, "op": "add", "block": block}).to_string();
        let r = ask(&sock, line).await;
        let err = r["error"].as_str().unwrap_or_default();
        assert!(r["ok"] != true && err.contains(want), "{block}: {r}");
    }
    // A port someone holds: the add is taken back.
    let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = held.local_addr().unwrap().port();
    let block = format!("module echo \"busy\" {{\n  version = \"2\"\n  port = {port}\n}}\n");
    let r = ask(
        &sock,
        serde_json::json!({"id": 2, "op": "add", "block": block}).to_string(),
    )
    .await;
    assert!(
        r["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("busy cannot listen on")),
        "{r}"
    );
    assert!(
        d.engine().get("busy").is_none(),
        "a refused add leaves nothing"
    );
    let r = ask(&sock, r#"{"id":3,"op":"remove","db":"e"}"#.into()).await;
    assert_eq!(
        r["error"], "instance \"e\" is needed by \"show\"; remove it first",
        "a remove of what another needs is refused: {r}"
    );
    s.stop();
    front.close();
    d.engine().shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn migrate_is_the_module_s_own_with_the_instance_stopped() {
    let (dir, _port) = project("migrate");
    let Some((d, s, sock, front)) = serve(&dir).await else {
        return;
    };
    let r = ask(&sock, r#"{"id":1,"op":"migrate","db":"show"}"#.into()).await;
    assert_eq!(
        r["error"],
        "show is process; its module has no migration (it keeps its data across versions, or has no such step)",
        "{r}"
    );
    let r = ask(&sock, r#"{"id":2,"op":"migrate","db":"e"}"#.into()).await;
    assert_eq!(
        r["error"], "e has no data yet: there is nothing to move",
        "{r}"
    );
    d.engine()
        .wake("e", doze_engine::Cause::Command)
        .await
        .unwrap();
    let r = ask(&sock, r#"{"id":3,"op":"migrate","db":"e"}"#.into()).await;
    assert_eq!(r["error"], "e is already on echo 2; nothing to move", "{r}");
    let marker = d.engine().stack().cluster_dir("e").join("ECHO_VERSION");
    std::fs::write(&marker, "1").unwrap();
    d.engine()
        .wake("e", doze_engine::Cause::Command)
        .await
        .unwrap();
    let r = ask(&sock, r#"{"id":4,"op":"migrate","db":"e"}"#.into()).await;
    assert_eq!(
        r["result"], "e is on echo 2 now (its echo 2.4.1 data was read)",
        "the module fetched during its migrate: {r}"
    );
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "2");
    let e = d.engine().get("e").unwrap();
    assert!(
        !e.state.running() && !e.off,
        "stopped for it, and wakes again: {e:?}"
    );
    let calls = std::fs::read_to_string(dir.join("calls")).unwrap_or_default();
    assert!(calls.lines().any(|l| l == "migrate"), "{calls}");
    s.stop();
    front.close();
    d.engine().shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lint_loads_the_file_on_disk_through_the_modules() {
    let (dir, _port) = project("lint");
    let Some((d, s, sock, front)) = serve(&dir).await else {
        return;
    };
    let r = ask(&sock, r#"{"id":1,"op":"lint"}"#.into()).await;
    assert_eq!(r["ok"], true, "{r}");
    let other = serde_json::json!({"id": 2, "op": "lint", "input": dir.join("x.hcl")}).to_string();
    let r = ask(&sock, other).await;
    assert!(
        r["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("this daemon serves")),
        "{r}"
    );
    // An edit the module refuses: the module's own check, at lint time, with the stack still up.
    let hcl = dir.join("doze.hcl");
    let text = std::fs::read_to_string(&hcl).unwrap();
    std::fs::write(&hcl, text.replacen("version = \"2\"", "version = \"7\"", 1)).unwrap();
    let r = ask(&sock, r#"{"id":3,"op":"lint"}"#.into()).await;
    assert!(
        r["ok"] != true
            && r["error"]
                .as_str()
                .is_some_and(|e| e.contains("is not a version the echo module serves")),
        "{r}"
    );
    // A type whose build cannot be had (the registry unreachable): unchecked, with why, and not a mistake.
    std::fs::write(&hcl, format!("{text}\nmodule offline \"o\" {{\n}}\n")).unwrap();
    let r = ask(&sock, r#"{"id":4,"op":"lint"}"#.into()).await;
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(
        r["unchecked"],
        serde_json::json!([{"type": "offline", "why": "fetching module index doze/offline/index.yaml: registry.test: no answer within 3s"}]),
        "{r}"
    );
    s.stop();
    front.close();
    d.engine().shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// After a remove: status does not name it, and a second remove says it is not there.
async fn gone_from_the_wire(sock: &std::path::Path) {
    let r = ask(sock, r#"{"id":3,"op":"status"}"#.into()).await;
    let names: Vec<&str> = r["instances"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["name"].as_str())
        .collect();
    assert!(!names.contains(&"late"), "{names:?}");
    let r = ask(sock, r#"{"id":4,"op":"remove","db":"late"}"#.into()).await;
    assert_eq!(
        (r["error"].as_str(), r["code"].as_str()),
        (Some("no instance named \"late\""), Some("not_found")),
        "{r}"
    );
}
