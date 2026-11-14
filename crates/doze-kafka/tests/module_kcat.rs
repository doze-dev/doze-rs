//! The module face against a real Kafka client (kcat, librdkafka), as an app reaches it under doze: through the address
//! doze grants, which the broker advertises in Metadata. The core's proxy is not in this workspace yet, so the test
//! stands in for it: a plain TCP forwarder on the granted address to the broker's backend.
//!
//! Needs `kcat` (or `kafkacat`); without it the test is skipped unless `DOZE_ACCEPTANCE` is set.
#![cfg(all(feature = "module", feature = "console", unix))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod module_support;

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::time::Duration;

use doze_module_proto::msg::instance::Address;
use module_support::harness;

fn kcat() -> Option<&'static str> {
    let found = ["kcat", "kafkacat"].into_iter().find(|p| {
        Command::new(p)
            .arg("-V")
            .output()
            .is_ok_and(|o| o.status.success())
    });
    assert!(
        found.is_some() || std::env::var_os("DOZE_ACCEPTANCE").is_none(),
        "DOZE_ACCEPTANCE is set: kcat is needed"
    );
    found
}

/// Accepts on `listen` and splices each connection to `to`, as doze's proxy does, for the rest of the test.
fn forward(listen: &str, to: String) {
    let l = TcpListener::bind(listen).unwrap();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let Ok(s) = TcpStream::connect(&to) else {
                continue;
            };
            let (mut c2, mut s2) = (c.try_clone().unwrap(), s.try_clone().unwrap());
            let (mut c, mut s) = (c, s);
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut c2, &mut s2);
                let _ = s2.shutdown(std::net::Shutdown::Write);
            });
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut s, &mut c);
                let _ = c.shutdown(std::net::Shutdown::Write);
            });
        }
    });
}

fn run(program: &str, args: &[&str], stdin: Option<&str>) -> String {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    if let Some(text) = stdin {
        input.write_all(text.as_bytes()).unwrap();
    }
    drop(input);
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{program} {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn kcat_lists_produces_and_consumes_through_the_granted_address() {
    let Some(kcat) = kcat() else { return };
    let b = harness().boot_example().await.unwrap();
    let main = &b.instance.listeners["main"];
    let Address::Tcp(backend) = &main.backend else {
        panic!("a TCP backend")
    };
    forward(&main.addr, backend.clone());
    let addr = main.addr.clone();
    let k = kcat.to_owned();
    let (meta, got) = tokio::task::spawn_blocking(move || {
        // The module's own shell line: `kcat -b {host}:{port} -L`.
        let meta = run(&k, &["-b", &addr, "-L", "-m", "10"], None);
        run(
            &k,
            &["-b", &addr, "-t", "orders", "-P", "-p", "0"],
            Some("one\ntwo\n"),
        );
        let got = run(
            &k,
            &[
                "-b",
                &addr,
                "-t",
                "orders",
                "-C",
                "-p",
                "0",
                "-o",
                "beginning",
                "-e",
                "-q",
                "-m",
                "10",
            ],
            None,
        );
        (meta, got)
    })
    .await
    .unwrap();
    assert!(
        meta.contains("topic \"orders\" with 3 partitions"),
        "{meta}"
    );
    assert!(
        meta.contains(&format!("at {}", main.addr)),
        "the broker advertises doze's address: {meta}"
    );
    assert_eq!(got.lines().collect::<Vec<_>>(), ["one", "two"]);
    b.stop(Duration::from_secs(20)).await;
}
