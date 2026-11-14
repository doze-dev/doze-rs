//! The daemon's frames against Go's, byte for byte (doze-control's `testdata/go/live`, recorded from Go's daemon): the
//! hello, the refusals of another protocol, the unary errors Go words, and a stream's acknowledgement and end.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code

use doze_daemon::{Daemon, StackInfo};
use doze_engine::Engine;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dzd-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn go_lines(file: &str) -> Vec<String> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../doze-control/testdata/go/live")
        .join(file);
    std::fs::read_to_string(p)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A raw connection: lines in, lines out.
struct Raw {
    w: doze_os::local::LocalStream,
    r: BufReader<doze_os::local::LocalStream>,
}

impl Raw {
    fn open(sock: &Path) -> Raw {
        let w = doze_os::local::LocalStream::connect(sock).unwrap();
        w.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let r = BufReader::new(w.try_clone().unwrap());
        Raw { w, r }
    }
    fn say(&mut self, line: &str) {
        self.w.write_all(format!("{line}\n").as_bytes()).unwrap();
    }
    fn hear(&mut self) -> String {
        let mut s = String::new();
        self.r.read_line(&mut s).unwrap();
        s.trim_end().to_owned()
    }
}

fn daemon(tag: &str) -> (Daemon, doze_daemon::Serving, PathBuf) {
    let dir = temp(tag);
    let e = Engine::builder("t", &dir).build().unwrap();
    let sock = dir.join("run/doze.admin.sock");
    let d = Daemon::at(e, BTreeMap::new(), StackInfo::default(), sock.clone());
    let s = d.serve().unwrap();
    (d, s, sock)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_hello_and_the_refusals_are_gos() {
    let (_d, s, sock) = daemon("hello");
    let go = go_lines("session.ndjson");
    let mut c = Raw::open(&sock);
    c.say(r#"{"hello":{"proto":2,"client":"cli"}}"#);
    assert_eq!(c.hear(), go[0]);
    let mut old = Raw::open(&sock);
    old.say(r#"{"op":"status"}"#);
    assert_eq!(old.hear(), go_lines("old-client.ndjson")[0]);
    let mut newer = Raw::open(&sock);
    newer.say(r#"{"hello":{"proto":3}}"#);
    assert_eq!(newer.hear(), go_lines("newer-client.ndjson")[0]);
    s.stop();
    assert!(!sock.exists(), "the socket goes with the server");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unary_errors_and_a_stream_are_worded_as_go() {
    let (_d, s, sock) = daemon("unary");
    let go = go_lines("session.ndjson");
    let mut c = Raw::open(&sock);
    c.say(r#"{"hello":{"proto":2}}"#);
    c.hear();
    let ask = |c: &mut Raw, req: &str| {
        c.say(req);
        c.hear()
    };
    let bad = ask(
        &mut c,
        r#"{"id":11,"op":"subscribe","topics":["stats"],"interval":"bogus"}"#,
    );
    assert_eq!(bad, go[2]);
    assert_eq!(ask(&mut c, r#"{"id":8,"op":"subscribe"}"#), go[3]);
    assert_eq!(ask(&mut c, r#"{"id":10,"op":"nope"}"#), go[5]);
    assert_eq!(ask(&mut c, r#"{"id":7,"op":"wake","db":"nosuch"}"#), go[7]);
    // A stream: acknowledged, then ended by its cancel with no error.
    assert_eq!(
        ask(&mut c, r#"{"id":9,"op":"subscribe","topics":["journal"]}"#),
        go[4]
    );
    c.say(r#"{"id":9,"op":"cancel"}"#);
    assert_eq!(
        c.hear(),
        r#"{"id":9,"at":"0001-01-01T00:00:00Z","end":true}"#
    );
    s.stop();
}

/// doze-control's own client drives the daemon: status, a verb, and the instances topic's first snapshot.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_control_client_reads_status() {
    let (_d, s, sock) = daemon("client");
    let client = doze_control::Client::socket(&sock);
    let got = tokio::task::spawn_blocking(move || client.dial().and_then(|c| c.snapshot()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.stack, "t");
    assert!(got.instances.is_empty());
    s.stop();
}
