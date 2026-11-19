//! Meters (Go's `Status` meters and `subStats`): a running service shows its resident memory, its tree's CPU and the
//! processes under it; the stats topic sends what was kept, then readings.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code

use doze_daemon::{Daemon, Labels, StackInfo};
use doze_engine::{Cause, Engine, ProcessSpec, Service};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

/// Not a test: the process the service runs. It starts a child (this binary again, as `leaf`) and waits.
#[test]
#[ignore = "run by meters_of_a_running_service as its service"]
fn sleeper() {
    let mut leaf = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "leaf", "--test-threads=1"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_secs(60));
    let _ = leaf.kill();
    let _ = leaf.wait();
}

/// Not a test: the child of [`sleeper`].
#[test]
#[ignore = "started by sleeper"]
fn leaf() {
    std::thread::sleep(Duration::from_secs(60));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn meters_of_a_running_service() {
    let dir = std::env::temp_dir().join(format!("dzd-meters-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let me = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let spec = ProcessSpec::new(
        [
            me.as_str(),
            "--ignored",
            "--exact",
            "sleeper",
            "--test-threads=1",
        ],
        &dir,
    );
    let e = Engine::builder("t", &dir)
        .service(Service::process("svc", spec))
        .build()
        .unwrap();
    let labels = BTreeMap::from([(
        "svc".to_string(),
        Labels {
            kind: "service".into(),
            ..Labels::default()
        },
    )]);
    let sock = dir.join("run/doze.admin.sock");
    let d = Daemon::at(e.clone(), labels, StackInfo::default(), sock.clone());
    let Some(s) = served(&d) else {
        e.shutdown().await;
        return;
    };
    e.wake("svc", Cause::Command).await.unwrap();
    // The leaf takes a moment to start.
    let mut got = None;
    for _ in 0..50 {
        let client = doze_control::Client::socket(&sock);
        let st = tokio::task::spawn_blocking(move || client.dial().and_then(|c| c.snapshot()))
            .await
            .unwrap()
            .unwrap();
        let v = st.instances.into_iter().find(|i| i.name == "svc").unwrap();
        if !v.children.is_empty()
            || doze_os::meters::sample(&[std::process::id()])
                .await
                .is_empty()
        {
            got = Some(v);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let v = got.expect("a view");
    if !doze_os::meters::sample(&[std::process::id()])
        .await
        .is_empty()
    {
        assert!(v.ram > 0, "{v:?}");
        assert!(v.cpu >= 0.0);
        assert_eq!(v.children.len(), 1, "{:?}", v.children);
        assert_eq!(v.children[0].depth, 1);
    }

    // The stats topic: readings with the service's meters and connections.
    let f = first_stats_frame(&sock);
    assert_eq!(f["topic"], "stats", "{f}");
    assert_eq!(f["stats"][0]["name"], "svc", "{f}");
    assert!(f["stats"][0]["pid"].as_i64().unwrap() > 0, "{f}");
    s.stop();
    e.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// The first frame of a stats subscription at 250 ms.
fn first_stats_frame(sock: &std::path::Path) -> serde_json::Value {
    let mut c = doze_os::local::LocalStream::connect(sock).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut r = BufReader::new(c.try_clone().unwrap());
    c.write_all(b"{\"hello\":{\"proto\":2}}\n{\"id\":3,\"op\":\"subscribe\",\"topics\":[\"stats\"],\"interval\":\"250ms\"}\n")
        .unwrap();
    // The hello's answer, the subscription's acknowledgement, then the first reading.
    let mut line = String::new();
    for _ in 0..3 {
        line.clear();
        r.read_line(&mut line).unwrap();
    }
    serde_json::from_str(&line).unwrap()
}

/// The daemon serving, or `None` where the system has no local control socket yet (Windows; the daemon there is W's).
fn served(d: &Daemon) -> Option<doze_daemon::Serving> {
    match d.serve() {
        Err(err) if err.kind() == std::io::ErrorKind::Unsupported => {
            eprintln!("skipped: {err}");
            None
        }
        s => Some(s.unwrap()),
    }
}
