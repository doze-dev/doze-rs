//! The Rust crate against the Go library it replaces (docs/port/names/PLAN.md N0), on the two things that must agree
//! for Go and Rust programs to share one machine's zone:
//!
//! - the address each name gets (recorded from the Go library in tests/fixtures/addresses.txt, so this half runs
//!   anywhere): compared as offsets into the block, which is what is the same on every system;
//! - the registry file itself: the Go library writes names, the Rust crate reads them and adds its own, and the Go
//!   library reads those back. That half needs Go and ../doze-names, so it says it skipped where they are missing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_names::pool::{LOOPBACK, Net, VIRTUAL};
use doze_names::{Name, Registry};
use std::io::{BufRead, BufReader};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("doze-names-oracle-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn every_name_gets_the_address_go_gives_it() {
    let fixture = include_str!("fixtures/addresses.txt");
    let mut n = 0;
    for line in fixture.lines() {
        let (host, go_ip) = line.split_once(' ').unwrap();
        let go_ip: Ipv4Addr = go_ip.parse().unwrap();
        // Recorded on a Mac (198.19) or Linux (127.0); the offset is the name's on either.
        let go_off = VIRTUAL
            .offset(go_ip)
            .or_else(|| LOOPBACK.offset(go_ip))
            .unwrap();
        let (svc, rest) = host.split_once('.').unwrap();
        let stack = rest.trim_end_matches(".doze");
        let home = scratch("addr");
        let lease = Registry::open(&home, "doze")
            .claim(&Name::qualified(svc, stack))
            .unwrap();
        assert_eq!(
            Net::native().offset(lease.ip),
            Some(go_off),
            "{host}: Go {go_ip}, Rust {}",
            lease.ip
        );
        n += 1;
    }
    assert_eq!(n, 36);
}

fn oracle_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/names-oracle");
    let go = Command::new("go")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success());
    (go && dir.join("../../../doze-names/names.go").exists()).then_some(dir)
}

#[test]
fn go_and_rust_share_one_registry_file() {
    let Some(dir) = oracle_dir() else {
        eprintln!("skipped: needs Go and ../doze-names beside doze-rs");
        return;
    };
    let home = scratch("file");
    // Go claims aws.doze and aws.harbour.doze (routed), and holds them while this test reads.
    let mut go = Command::new("go")
        .args(["run", ".", "registry"])
        .arg(&home)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pid = String::new();
    BufReader::new(go.stdout.take().unwrap())
        .read_line(&mut pid)
        .unwrap();
    assert!(
        pid.trim().parse::<u32>().is_ok(),
        "the Go side said {pid:?}"
    );

    let rust = Registry::open(&home, "doze-kafka");
    let aws = rust.resolve("aws.doze").expect("Rust reads Go's apex name");
    assert_eq!(Net::native().offset(aws), Some(2));
    let harbour = rust
        .resolve("sqs.ap-south-1.aws.harbour.doze")
        .expect("and its subtree");
    assert_eq!(
        rust.front_target("aws.harbour.doze").as_deref(),
        Some("127.0.0.4:4566")
    );
    let _kafka = rust.claim(&Name::apex("kafka")).unwrap();
    let _db = rust.claim(&Name::qualified("db", "shop")).unwrap();

    let read = Command::new("go")
        .args(["run", ".", "read"])
        .arg(&home)
        .current_dir(&dir)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&read.stdout).to_string();
    drop(go.stdin.take());
    let _ = go.wait();
    let line = |h: &str| {
        text.lines()
            .find(|l| l.starts_with(&format!("{h} ")))
            .unwrap_or_default()
            .to_string()
    };
    assert!(
        line("kafka.doze").ends_with(&Net::native().at(3).to_string()),
        "Go reads Rust's apex name:\n{text}"
    );
    assert!(
        !line("db.shop.doze").ends_with("<nil>"),
        "Go reads Rust's qualified name:\n{text}"
    );
    assert!(
        line("sqs.ap-south-1.aws.harbour.doze").ends_with(&harbour.to_string()),
        "{text}"
    );
}
