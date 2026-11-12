//! valkey on the real valkey-server (`DOZE_VALKEY_BINDIR`, from doze-binaries). Each test is a promise the module
//! makes, checked on the engine: the published example needs its password and applies its settings; a cache that
//! sleeps wakes with its keys (and `save = ""` really keeps nothing); the app sees the server's numbers, can empty it,
//! and watches commands live; a mistake is refused before boot, at its line. Valkey ships for macOS and Linux only.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_proto::msg::calls::RunAction;
use doze_module_proto::msg::console::{Row, TailArgs};
use doze_module_proto::msg::instance::Address;
use doze_module_proto::msg::run::WithToolchain;
use doze_module_test::{Booted, Harness, Stops, acceptance};
use doze_os::tree::Stopped;
use doze_resp::{Client, Reply};
use std::time::Duration;
use tokio::net::UnixStream;

fn harness() -> Option<Harness> {
    acceptance(env!("CARGO_BIN_EXE_doze-module-valkey"), "valkey").unwrap()
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
        Reply::Integer(n) => n.to_string(),
        Reply::Array(Some(items)) => items.iter().map(text).collect::<Vec<_>>().join(" "),
        other => other.text().unwrap_or_else(|| format!("{other:?}")),
    }
}

async fn call(c: &mut Client<UnixStream>, args: &[&str]) -> String {
    text(&c.call(args).await.unwrap())
}

fn graceful(stops: &Stops) {
    assert!(
        !stops.is_empty()
            && stops
                .iter()
                .all(|(_, r)| matches!(r, Ok(Stopped::Graceful(st)) if st.success())),
        "{stops:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_published_example_needs_its_password_and_applies_its_settings() {
    let Some(h) = harness() else { return };
    let b = h.boot_example().await.unwrap();
    let mut c = client(&b).await;
    let refused = call(&mut c, &["SET", "k", "v"]).await;
    assert!(refused.starts_with("-NOAUTH"), "{refused}");
    assert_eq!(call(&mut c, &["AUTH", "cache"]).await, "OK");
    assert_eq!(call(&mut c, &["SET", "k", "v"]).await, "OK");
    for (key, want) in [
        ("maxmemory-policy", "allkeys-lru"),
        ("maxmemory", "268435456"),
        ("appendonly", "yes"),
        ("save", "900 1 300 10"),
        ("tcp-keepalive", "60"),
    ] {
        let got = call(&mut c, &["CONFIG", "GET", key]).await;
        assert_eq!(got, format!("{key} {want}"));
    }
    // The password is in a file only the user can read, and not on the command line `ps` shows.
    let with = WithToolchain {
        instance: b.instance.clone(),
        toolchain: b.toolchain.clone(),
    };
    let args = b.module.plan(&with).await.unwrap().processes[0]
        .args
        .clone();
    assert!(
        !args.iter().any(|a| a == "--requirepass" || a == "cache"),
        "{args:?}"
    );
    let auth = std::path::Path::new(&b.instance.data_dir).join("doze-auth.conf");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&auth).unwrap().permissions().mode() & 0o777,
        0o600
    );
    graceful(&b.stop(Duration::from_secs(10)).await);
}

#[tokio::test(flavor = "current_thread")]
async fn a_cache_that_sleeps_wakes_with_its_keys() {
    let Some(h) = harness() else { return };
    let b = h
        .boot("valkey \"keeps\" {\n  version = 9\n}\n")
        .await
        .unwrap();
    assert_eq!(
        call(&mut client(&b).await, &["SET", "k", "kept"]).await,
        "OK"
    );
    let (stops, b) = b.sleep_and_wake(Duration::from_secs(10)).await.unwrap();
    graceful(&stops);
    assert!(!b.fresh, "waking reuses the data directory");
    assert_eq!(call(&mut client(&b).await, &["GET", "k"]).await, "kept");
    graceful(&b.stop(Duration::from_secs(10)).await);
}

#[tokio::test(flavor = "current_thread")]
async fn save_empty_means_memory_only() {
    let Some(h) = harness() else { return };
    let b = h
        .boot("valkey \"forgets\" {\n  version = 9\n  save = \"\"\n}\n")
        .await
        .unwrap();
    assert_eq!(call(&mut client(&b).await, &["SET", "k", "v"]).await, "OK");
    let (_, b) = b.sleep_and_wake(Duration::from_secs(10)).await.unwrap();
    assert_eq!(call(&mut client(&b).await, &["GET", "k"]).await, "(nil)");
    graceful(&b.stop(Duration::from_secs(10)).await);
}

#[tokio::test(flavor = "current_thread")]
async fn the_app_sees_the_numbers_watches_commands_and_can_empty_it() {
    let Some(h) = harness() else { return };
    let b = h
        .boot("valkey \"seen\" {\n  version = 9\n  maxmemory = \"64mb\"\n  password = \"p@ss:/w\"\n}\n")
        .await
        .unwrap();
    let mut c = client(&b).await;
    assert_eq!(call(&mut c, &["AUTH", "p@ss:/w"]).await, "OK");
    for k in ["a", "b", "c"] {
        assert_eq!(call(&mut c, &["SET", k, "1"]).await, "OK");
    }
    let res = b.module.resources(&b.instance).await.unwrap();
    let server = res.iter().find(|r| r.kind == "server").unwrap();
    assert_eq!(server.info["keys"], "3");
    assert_eq!(server.info["ceiling"], "64 MB");
    assert_eq!(server.info["persistence"], "snapshot when it sleeps");
    assert!(
        res.iter()
            .any(|r| r.kind == "database" && r.name == "db0" && r.info["keys"] == "3")
    );

    // A command sent by one client shows up in the app's stream.
    let args = TailArgs {
        panel: "commands".into(),
        selector: String::new(),
        instance: b.instance.clone(),
    };
    let mut stream = b.module.tail(&args).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(call(&mut c, &["GET", "watched"]).await, "(nil)");
    let row: Row = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        row.cells["command"].contains("\"GET\" \"watched\""),
        "{row:?}"
    );
    drop(stream);

    let act = |action: &str| RunAction {
        instance: b.instance.clone(),
        action: action.into(),
        resource: "valkey".into(),
        input: None,
    };
    assert_eq!(
        b.module.run_action(&act("save")).await.unwrap(),
        "saving a snapshot"
    );
    assert_eq!(b.module.run_action(&act("flush")).await.unwrap(), "emptied");
    assert_eq!(call(&mut c, &["DBSIZE"]).await, "0");
    assert!(b.module.run_action(&act("drop-everything")).await.is_err());
    graceful(&b.stop(Duration::from_secs(10)).await);
}

/// The module refuses only what valkey itself refuses: each value the oracle shows refused early is given to the real
/// server, which must exit at once over its config.
#[tokio::test(flavor = "current_thread")]
async fn what_the_module_refuses_the_engine_refuses_too() {
    let Some(h) = harness() else { return };
    let server = h.bin_dir.join("valkey-server");
    for (flag, value) in [
        ("--maxmemory-policy", "256MB"),
        ("--save", "value-4"),
        ("--maxmemory", "256 megs"),
    ] {
        let out = std::process::Command::new(&server)
            .args(["--port", "0", "--save", "", flag, value])
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stderr).to_string()
            + &String::from_utf8_lossy(&out.stdout);
        assert!(
            !out.status.success(),
            "valkey accepted {flag} {value:?}: {said}"
        );
        assert!(said.contains("FATAL CONFIG"), "{flag} {value:?}: {said}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_mistake_is_refused_before_boot_with_a_way_out() {
    let Some(h) = harness() else { return };
    let e = h
        .boot("valkey \"typo\" {\n  version = 9\n  maxmemory_policy = \"allkeys-lur\"\n}\n")
        .await
        .unwrap_err();
    assert!(
        e.contains("maxmemory_policy") || e.contains("allkeys-lur"),
        "{e}"
    );
    assert!(e.contains("allkeys-lru"), "the way out is named: {e}");
}

/// The shell client the manifest names is in the engine's own build, and its rendered environment logs in: the
/// password reaches it through `REDISCLI_AUTH`, never its command line.
#[tokio::test(flavor = "current_thread")]
async fn the_declared_client_is_in_the_build_and_logs_in() {
    let Some(h) = harness() else { return };
    let b = h
        .boot("valkey \"cli\" {\n  version = 9\n  password = \"p@ss word\"\n}\n")
        .await
        .unwrap();
    let client = b.module.manifest.client.clone().unwrap();
    let program = h.bin_dir.join(&client.program);
    assert!(program.is_file(), "{} is not in the build", client.program);
    let vars = std::collections::BTreeMap::from([("password", "p@ss word".to_string())]);
    let (_, env) = client.render(&vars);
    assert!(
        !client.args.iter().any(|a| a.contains("{password}")),
        "a secret in args: {:?}",
        client.args
    );
    let Some(Address::Unix(socket)) = b.backend() else {
        panic!()
    };
    let out = std::process::Command::new(&program)
        .args(["-s", &socket, "PING"])
        .envs(&env)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "PONG",
        "{out:?}"
    );
    graceful(&b.stop(Duration::from_secs(10)).await);
}
