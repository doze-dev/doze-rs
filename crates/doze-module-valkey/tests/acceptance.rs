//! valkey on the real valkey-server (`DOZE_VALKEY_BINDIR`, from doze-binaries): the published example boots, refuses a
//! client without the password and serves one with it, keeps the password off the command line, applies the typed
//! settings, and stops gracefully. Valkey ships for macOS and Linux only (no Windows build), so this is Unix-only.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_proto::msg::instance::Address;
use doze_module_proto::msg::run::WithToolchain;
use doze_module_test::{Booted, Harness, acceptance};
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
        Reply::Array(Some(items)) => items.iter().map(text).collect::<Vec<_>>().join(" "),
        other => other.text().unwrap_or_else(|| format!("{other:?}")),
    }
}

async fn stop_gracefully(b: Booted) {
    let stops = b.stop(Duration::from_secs(10)).await;
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
    let refused = text(&c.call(&["SET", "k", "v"]).await.unwrap());
    assert!(refused.starts_with("-NOAUTH"), "{refused}");
    assert_eq!(text(&c.call(&["AUTH", "cache"]).await.unwrap()), "OK");
    assert_eq!(text(&c.call(&["SET", "k", "v"]).await.unwrap()), "OK");
    assert_eq!(text(&c.call(&["GET", "k"]).await.unwrap()), "v");
    for (key, want) in [
        ("maxmemory-policy", "allkeys-lru"),
        ("maxmemory", "268435456"),
        ("appendonly", "yes"),
        ("save", "900 1 300 10"),
        ("tcp-keepalive", "60"),
    ] {
        let got = text(&c.call(&["CONFIG", "GET", key]).await.unwrap());
        assert_eq!(got, format!("{key} {want}"));
    }
    // The password is in a file only the user can read, and not on the command line `ps` shows.
    let with = WithToolchain {
        instance: b.instance.clone(),
        toolchain: b.toolchain.clone(),
    };
    let plan = b.module.plan(&with).await.unwrap();
    let args = &plan.processes[0].args;
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
    stop_gracefully(b).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_bare_block_answers_without_a_password() {
    let Some(h) = harness() else { return };
    let b = h
        .boot("valkey \"bare\" {\n  version = 9\n}\n")
        .await
        .unwrap();
    assert!(b.fresh);
    let mut c = client(&b).await;
    assert_eq!(text(&c.call(&["PING"]).await.unwrap()), "PONG");
    assert_eq!(
        text(&c.call(&["CONFIG", "GET", "save"]).await.unwrap()),
        "save "
    );
    stop_gracefully(b).await;
}
