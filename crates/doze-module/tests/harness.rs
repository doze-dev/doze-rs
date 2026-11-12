//! The module test harness on the echo module and its real engine: boot the published example in a sandbox, talk to
//! the engine on the backend the harness granted (a socket on Unix, loopback TCP on Windows), stop it gracefully.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_proto::msg::instance::Address;
use doze_module_test::{Booted, Harness};
use doze_os::tree::Stopped;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn harness() -> Harness {
    let engine = PathBuf::from(env!("CARGO_BIN_EXE_doze-module-echo-engine"));
    Harness {
        program: env!("CARGO_BIN_EXE_doze-module-echo").into(),
        engine: "echo".into(),
        bin_dir: engine.parent().unwrap().to_path_buf(),
        full: "2.0.0".into(),
    }
}

/// One line out, the same line back, over whatever backend the instance was given.
async fn echo_once(b: &Booted, text: &str) -> String {
    let line = format!("{text}\n");
    let mut got = String::new();
    match b.backend().expect("a main backend") {
        Address::Tcp(addr) => {
            let mut s = BufReader::new(tokio::net::TcpStream::connect(addr).await.unwrap());
            s.get_mut().write_all(line.as_bytes()).await.unwrap();
            s.read_line(&mut got).await.unwrap();
        }
        #[cfg(unix)]
        Address::Unix(path) => {
            let mut s = BufReader::new(tokio::net::UnixStream::connect(path).await.unwrap());
            s.get_mut().write_all(line.as_bytes()).await.unwrap();
            s.read_line(&mut got).await.unwrap();
        }
        #[cfg(not(unix))]
        Address::Unix(_) => panic!("a socket backend on a system without Unix sockets"),
    }
    got.trim_end().to_string()
}

fn all_graceful(stops: &[(String, std::io::Result<Stopped>)]) -> bool {
    !stops.is_empty()
        && stops
            .iter()
            .all(|(_, r)| matches!(r, Ok(Stopped::Graceful(st)) if st.success()))
}

#[tokio::test(flavor = "current_thread")]
async fn the_published_example_boots_answers_and_stops_gracefully() {
    let booted = harness().boot_example().await.unwrap();
    assert!(booted.fresh, "a new sandbox provisions fresh");
    assert_eq!(echo_once(&booted, "hello").await, "hello");
    let stops = booted.stop(Duration::from_secs(10)).await;
    assert!(all_graceful(&stops), "{stops:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_block_with_settings_and_channels_boots() {
    let hcl = "echo \"chat\" {\n  version = \"2\"\n  greeting = \"hi\"\n  buffer = 32\n  settings = { mode = \"fast\" }\n  channel \"a\" { size = 4 }\n}\n";
    let booted = harness().boot(hcl).await.unwrap();
    assert_eq!(booted.instance.name, "chat");
    assert_eq!(echo_once(&booted, "with channels").await, "with channels");
    assert!(all_graceful(&booted.stop(Duration::from_secs(10)).await));
}

#[tokio::test(flavor = "current_thread")]
async fn a_bad_block_fails_with_the_field_that_is_wrong() {
    let hcl = "echo \"bad\" {\n  version = \"2\"\n  channel \"a\" { size = 0 }\n}\n";
    let e = harness().boot(hcl).await.unwrap_err();
    assert!(
        e.contains("at least one message") && e.contains("Index(0)"),
        "{e}"
    );
}
