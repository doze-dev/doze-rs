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
        // The line the module declares as its shell client (module/hints.rs): `kcat -b {host}:{port} -L`.
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

/// What `kcat` reads from the start of every partition of `topic`, as `partition:offset:value` lines, sorted.
fn read_all(kcat: &str, addr: &str, topic: &str) -> Vec<String> {
    let out = run(
        kcat,
        &[
            "-b",
            addr,
            "-t",
            topic,
            "-C",
            "-o",
            "beginning",
            "-e",
            "-q",
            "-f",
            "%p:%o:%s\\n",
        ],
        None,
    );
    let mut lines: Vec<String> = out.lines().map(str::to_owned).collect();
    lines.sort();
    lines
}

/// An instance folder the Go doze-kafka module left (tests/fixtures/go-data, written by the Go broker with kcat) is taken
/// over at provision: the records keep their offsets, the topics their ids and partitions, the groups their committed
/// offsets; converge then finds the declared topic there, and new records follow the old ones.
#[tokio::test(flavor = "multi_thread")]
async fn the_go_modules_data_is_taken_over() {
    let Some(kcat) = kcat() else { return };
    let seed = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/go-data");
    let hcl = "kafka \"events\" {\n  version = 4\n  auto_create_topics = false\n  topic \"orders.placed\" {\n    partitions = 3\n  }\n}\n";
    let b = harness()
        .boot_seeded(hcl, doze_module_test::Boot::default(), &seed)
        .await
        .unwrap();
    let data = std::path::PathBuf::from(&b.instance.data_dir);
    assert!(data.join("meta.redb").is_file() && data.join("meta.db.go").is_file());
    let with = doze_module_proto::msg::run::WithToolchain {
        instance: b.instance.clone(),
        toolchain: b.toolchain.clone(),
    };
    b.module.converge(&with).await.unwrap();
    let main = &b.instance.listeners["main"];
    let Address::Tcp(backend) = &main.backend else {
        panic!("a TCP backend")
    };
    forward(&main.addr, backend.clone());
    let addr = main.addr.clone();
    let k = kcat.to_owned();
    let seen = tokio::task::spawn_blocking(move || look(&k, &addr))
        .await
        .unwrap();
    seen.check();
    b.stop(Duration::from_secs(20)).await;
}

/// What kcat sees of the taken-over broker.
struct Seen {
    meta: String,
    orders: Vec<String>,
    payments: Vec<String>,
    signed: Vec<String>,
    group: String,
    after: Vec<String>,
}

fn look(k: &str, addr: &str) -> Seen {
    let meta = run(k, &["-b", addr, "-L", "-m", "10"], None);
    let orders = read_all(k, addr, "orders.placed");
    let payments = read_all(k, addr, "payments");
    let signed = read_all(k, addr, "users.signed-up");
    // The group `shippers` committed 3 in partition 1 and 1 in partition 2: it resumes there.
    let group = run(
        k,
        &[
            "-b",
            addr,
            "-C",
            "-e",
            "-q",
            "-f",
            "%p:%o:%s\\n",
            "-X",
            "auto.offset.reset=earliest",
            "-G",
            "shippers",
            "orders.placed",
        ],
        None,
    );
    run(
        k,
        &["-b", addr, "-t", "payments", "-P", "-p", "0"],
        Some("y:4\n"),
    );
    let after = read_all(k, addr, "payments");
    Seen {
        meta,
        orders,
        payments,
        signed,
        group,
        after,
    }
}

impl Seen {
    /// What the Go broker serves from the same folder, and a new record after it.
    fn check(self) {
        let Seen {
            meta,
            orders,
            payments,
            signed,
            group,
            after,
        } = self;
        for t in ["orders.placed", "payments", "users.signed-up"] {
            assert!(
                meta.contains(&format!("topic \"{t}\" with 3 partitions")),
                "{meta}"
            );
        }
        assert_eq!(
            orders,
            [
                "0:0:one",
                "0:1:three",
                "0:2:four",
                "1:0:order-1",
                "1:1:order-2",
                "1:2:order-3",
                "1:3:order-4",
                "1:4:order-5",
                "1:5:order-6",
                "2:0:two",
            ]
            .map(String::from)
            .to_vec(),
            "every record Go held, at its offset (what the Go broker serves from the same folder)"
        );
        assert_eq!(
            payments,
            ["0:0:y:1", "0:1:y:2", "0:2:y:3"].map(String::from).to_vec()
        );
        assert_eq!(signed, ["1:0:solo"].map(String::from).to_vec());
        let mut group: Vec<&str> = group.lines().collect();
        group.sort_unstable();
        assert_eq!(
            group,
            [
                "0:0:one",
                "0:1:three",
                "0:2:four",
                "1:3:order-4",
                "1:4:order-5",
                "1:5:order-6"
            ],
            "the group resumes where Go had it"
        );
        assert_eq!(
            after.last().map(String::as_str),
            Some("0:3:y:4"),
            "a new record follows Go's"
        );
    }
}
