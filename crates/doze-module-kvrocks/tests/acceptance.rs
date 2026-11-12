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
