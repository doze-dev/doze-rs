//! kvrocks on the real kvrocks server (`DOZE_KVROCKS_BINDIR`, from doze-binaries): the published example boots, needs
//! its password, keeps each namespace's keys apart, applies its settings and stops gracefully; a bare block answers
//! without one. Kvrocks ships for macOS and Linux only (no Windows build), so this is Unix-only.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_proto::msg::console::{Resource, Row, TailArgs};
use doze_module_proto::msg::instance::Address;
use doze_module_test::{Booted, Harness, acceptance};
use doze_os::tree::Stopped;
use doze_resp::{Client, Reply};
use std::time::Duration;
use tokio::net::UnixStream;

fn harness() -> Option<Harness> {
    acceptance(env!("CARGO_BIN_EXE_doze-module-kvrocks"), "kvrocks").unwrap()
}

async fn client(b: &Booted) -> Client<UnixStream> {
    let Some(Address::Unix(path)) = b.backend() else {
        panic!("a socket backend on Unix, got {:?}", b.backend())
    };
    Client::new(UnixStream::connect(path).await.unwrap())
}

fn text(r: &Reply) -> String {
    match r {
        Reply::Error(e) => format!("-{e}"),
        Reply::Bulk(None) => "(nil)".into(),
        Reply::Array(Some(items)) => items.iter().map(text).collect::<Vec<_>>().join(" "),
        other => other.text().unwrap_or_else(|| format!("{other:?}")),
    }
}

async fn call(c: &mut Client<UnixStream>, args: &[&str]) -> String {
    text(&c.call(args).await.unwrap())
}

async fn stop_gracefully(b: Booted) {
    let stops = b.stop(Duration::from_secs(20)).await;
    assert!(
        !stops.is_empty()
            && stops
                .iter()
                .all(|(_, r)| matches!(r, Ok(Stopped::Graceful(st)) if st.success())),
        "{stops:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_published_example_needs_its_password_and_keeps_namespaces_apart() {
    let Some(h) = harness() else { return };
    let b = h.boot_example().await.unwrap();
    let mut admin = client(&b).await;
    let refused = call(&mut admin, &["SET", "k", "v"]).await;
    assert!(refused.starts_with("-NOAUTH"), "{refused}");
    assert_eq!(call(&mut admin, &["AUTH", "admin"]).await, "OK");
    let mut tenant = client(&b).await;
    assert_eq!(call(&mut tenant, &["AUTH", "tenant-a-token"]).await, "OK");
    assert_eq!(call(&mut tenant, &["SET", "k", "tenant"]).await, "OK");
    assert_eq!(call(&mut tenant, &["GET", "k"]).await, "tenant");
    assert_eq!(
        call(&mut admin, &["GET", "k"]).await,
        "(nil)",
        "namespaces must not share keys"
    );
    assert_eq!(
        call(&mut admin, &["CONFIG", "GET", "workers"]).await,
        "workers 4"
    );
    assert_eq!(
        call(&mut admin, &["CONFIG", "GET", "rocksdb.block_cache_size"]).await,
        "rocksdb.block_cache_size 256"
    );
    stop_gracefully(b).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_bare_block_answers_without_a_password() {
    let Some(h) = harness() else { return };
    let b = h
        .boot("kvrocks \"bare\" {\n  version = 2\n}\n")
        .await
        .unwrap();
    assert!(b.fresh);
    let mut c = client(&b).await;
    assert_eq!(call(&mut c, &["PING"]).await, "PONG");
    assert_eq!(call(&mut c, &["SET", "k", "v"]).await, "OK");
    assert_eq!(call(&mut c, &["GET", "k"]).await, "v");
    stop_gracefully(b).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_store_that_sleeps_wakes_with_every_namespace_intact() {
    let Some(h) = harness() else { return };
    let b = h.boot_example().await.unwrap();
    let mut admin = client(&b).await;
    assert_eq!(call(&mut admin, &["AUTH", "admin"]).await, "OK");
    assert_eq!(call(&mut admin, &["SET", "k", "admin's"]).await, "OK");
    let mut tenant = client(&b).await;
    assert_eq!(call(&mut tenant, &["AUTH", "tenant-b-token"]).await, "OK");
    assert_eq!(call(&mut tenant, &["SET", "k", "b's"]).await, "OK");
    let (stops, b) = b.sleep_and_wake(Duration::from_secs(20)).await.unwrap();
    assert!(
        stops
            .iter()
            .all(|(_, r)| matches!(r, Ok(Stopped::Graceful(_)))),
        "{stops:?}"
    );
    let mut admin = client(&b).await;
    assert_eq!(call(&mut admin, &["AUTH", "admin"]).await, "OK");
    assert_eq!(call(&mut admin, &["GET", "k"]).await, "admin's");
    let mut tenant = client(&b).await;
    assert_eq!(call(&mut tenant, &["AUTH", "tenant-b-token"]).await, "OK");
    assert_eq!(call(&mut tenant, &["GET", "k"]).await, "b's");
    stop_gracefully(b).await;
}

fn keys_of(res: &[Resource], ns: &str) -> Option<String> {
    res.iter()
        .find(|r| r.kind == "namespace" && r.name == ns)
        .map(|r| r.info["keys"].clone())
}

/// The app's resources once namespace `ns` shows `want` keys. kvrocks counts lazily: a look starts a recount, a later
/// look sees it.
async fn counted(b: &Booted, ns: &str, want: &str) -> Vec<Resource> {
    let mut res = b.module.resources(&b.instance).await.unwrap();
    for _ in 0..20 {
        if keys_of(&res, ns).as_deref() == Some(want) {
            return res;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        res = b.module.resources(&b.instance).await.unwrap();
    }
    panic!("{ns} never showed {want} keys: {res:#?}")
}

/// The row the app's command stream shows for a `GET watched` sent by `c`.
async fn watched(b: &Booted, c: &mut Client<UnixStream>) -> Row {
    let args = TailArgs {
        panel: "commands".into(),
        selector: String::new(),
        instance: b.instance.clone(),
    };
    let mut stream = b.module.tail(&args).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(call(c, &["GET", "watched"]).await, "(nil)");
    let row: Row = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        row.cells["command"].contains("\"GET\" \"watched\""),
        "{row:?}"
    );
    row
}

#[tokio::test(flavor = "current_thread")]
async fn the_app_sees_each_namespace_watches_commands_and_can_empty_one() {
    use doze_module_proto::msg::calls::RunAction;
    let Some(h) = harness() else { return };
    let b = h.boot_example().await.unwrap();
    let mut a = client(&b).await;
    assert_eq!(call(&mut a, &["AUTH", "tenant-a-token"]).await, "OK");
    for k in ["x", "y"] {
        assert_eq!(call(&mut a, &["SET", k, "1"]).await, "OK");
    }
    let mut admin = client(&b).await;
    assert_eq!(call(&mut admin, &["AUTH", "admin"]).await, "OK");
    assert_eq!(call(&mut admin, &["SET", "z", "1"]).await, "OK");

    let res = counted(&b, "tenant_a", "2").await;
    assert_eq!(keys_of(&res, "(default)").as_deref(), Some("1"), "{res:#?}");
    assert_eq!(keys_of(&res, "tenant_b").as_deref(), Some("0"), "{res:#?}");
    let server = res.iter().find(|r| r.kind == "server").unwrap();
    assert!(server.info["version"].starts_with("2."), "{server:?}");
    assert!(server.info["disk"].ends_with('B'), "{server:?}");
    let row = watched(&b, &mut a).await;
    assert!(row.cells["client"].starts_with("tenant_a"), "{row:?}");

    let act = |action: &str, resource: &str| RunAction {
        instance: b.instance.clone(),
        action: action.into(),
        resource: resource.into(),
        input: None,
    };
    assert_eq!(
        b.module
            .run_action(&act("flush", "tenant_a"))
            .await
            .unwrap(),
        "emptied tenant_a"
    );
    assert_eq!(call(&mut a, &["GET", "x"]).await, "(nil)");
    assert_eq!(
        call(&mut admin, &["GET", "z"]).await,
        "1",
        "emptying one namespace leaves the others"
    );
    assert_eq!(
        b.module
            .run_action(&act("compact", "kvrocks"))
            .await
            .unwrap(),
        "compacting"
    );
    assert!(b.module.run_action(&act("flush", "nobody")).await.is_err());
    stop_gracefully(b).await;
}

/// What the module refuses, checked against kvrocks itself: it refuses `workers 0` and a token equal to the password
/// (but only at boot), and it starts with two namespaces sharing a token, which the module refuses for the tenants'
/// sake.
#[tokio::test(flavor = "current_thread")]
async fn what_the_module_refuses_kvrocks_refuses_or_gets_wrong() {
    let Some(h) = harness() else { return };
    let dir = std::env::temp_dir().join(format!("dzkv-{}", std::process::id()));
    for (lines, starts) in [
        ("workers 0", false),
        ("namespace.a admin", false),
        ("namespace.a same\nnamespace.b same", true),
    ] {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let port = doze_module_test::sandbox::free_port().unwrap();
        let conf = format!(
            "dir {}\nbind 127.0.0.1\nport {port}\nrequirepass admin\n{lines}\n",
            dir.display()
        );
        std::fs::write(dir.join("k.conf"), conf).unwrap();
        let mut child = std::process::Command::new(h.bin_dir.join("kvrocks"))
            .arg("-c")
            .arg(dir.join("k.conf"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
        let running = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(running, starts, "kvrocks with {lines:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
    for (hcl, says) in [
        ("workers = 0", "worker"),
        (
            "password = \"admin\"\n  namespace \"a\" {\n    token = \"admin\"\n  }",
            "password",
        ),
        (
            "password = \"admin\"\n  namespace \"a\" {\n    token = \"same\"\n  }\n  namespace \"b\" {\n    token = \"same\"\n  }",
            "same token",
        ),
    ] {
        let e = h
            .boot(&format!("kvrocks \"r\" {{\n  version = 2\n  {hcl}\n}}\n"))
            .await
            .unwrap_err();
        assert!(e.contains(says), "{hcl:?}: {e}");
    }
}
