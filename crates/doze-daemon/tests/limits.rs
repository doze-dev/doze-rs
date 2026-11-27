//! The control channel on the daemon's runtime: a client that stops reading holds back neither another client nor the
//! runtime's workers (what supervises every service), and is disconnected; a connection runs at most so many requests
//! and streams; the daemon takes at most so many connections; stopping ends the accept at once.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code

use doze_daemon::{Daemon, Limits, StackInfo};
use doze_engine::Engine;
use doze_os::local::LocalStream;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dzl-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `None` where the system has no local control socket yet (Windows).
fn daemon(tag: &str, limits: Limits) -> Option<(Daemon, doze_daemon::Serving, PathBuf)> {
    let dir = temp(tag);
    let e = Engine::builder("t", &dir).build().unwrap();
    let sock = dir.join("run/doze.admin.sock");
    let d = Daemon::at(e, BTreeMap::new(), StackInfo::default(), sock.clone()).with_limits(limits);
    match d.serve() {
        Err(err) if err.kind() == std::io::ErrorKind::Unsupported => {
            eprintln!("skipped: {err}");
            None
        }
        s => Some((d, s.unwrap(), sock)),
    }
}

/// A raw connection: lines in, frames out.
struct Raw {
    w: LocalStream,
    r: BufReader<LocalStream>,
}

impl Raw {
    fn open(sock: &Path) -> Raw {
        let w = LocalStream::connect(sock).unwrap();
        w.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let r = BufReader::new(w.try_clone().unwrap());
        Raw { w, r }
    }
    fn greeted(sock: &Path) -> Raw {
        let mut c = Raw::open(sock);
        c.say(r#"{"hello":{"proto":2,"client":"test"}}"#);
        let hello = c.hear();
        assert!(hello["hello"]["proto"] == 2, "{hello}");
        c
    }
    fn say(&mut self, line: &str) {
        self.w.write_all(format!("{line}\n").as_bytes()).unwrap();
    }
    fn hear(&mut self) -> serde_json::Value {
        let mut s = String::new();
        self.r.read_line(&mut s).unwrap();
        serde_json::from_str(s.trim_end()).unwrap_or_else(|e| panic!("{e}: {s:?}"))
    }
    /// The answer to request `id` (a stream's acknowledgement or refusal), skipping other frames.
    fn answer(&mut self, id: u64) -> serde_json::Value {
        loop {
            let f = self.hear();
            if f["id"] == id && f.get("response").is_some() {
                return f["response"].clone();
            }
        }
    }
    /// Whether the daemon closes the connection within `within`, reading (and dropping) what it sent first.
    fn closed_within(&mut self, within: Duration) -> bool {
        let until = Instant::now() + within;
        // macOS refuses the option on a socket the other side has shut down: closed.
        if self
            .r
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(200)))
            .is_err()
        {
            return true;
        }
        let mut buf = vec![0u8; 64 * 1024];
        while Instant::now() < until {
            match self.r.read(&mut buf) {
                Ok(0) => return true,
                Ok(_) => {}
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => return true,
            }
        }
        false
    }
}

/// How late a 10 ms tick on the runtime runs, at most, while `probe` lives: a worker held by blocking work shows here
/// (the engine's supervision, its timers and its child watches, runs on the same two workers).
struct Lag {
    worst_us: Arc<AtomicU64>,
    task: tokio::task::JoinHandle<()>,
}

impl Lag {
    fn probe() -> Lag {
        let worst_us = Arc::new(AtomicU64::new(0));
        let w = Arc::clone(&worst_us);
        let task = tokio::spawn(async move {
            let tick = Duration::from_millis(10);
            loop {
                let at = Instant::now();
                tokio::time::sleep(tick).await;
                let late = at.elapsed().saturating_sub(tick);
                w.fetch_max(u64::try_from(late.as_micros()).unwrap(), Ordering::SeqCst);
            }
        });
        Lag { worst_us, task }
    }
    fn worst(self) -> Duration {
        self.task.abort();
        Duration::from_micros(self.worst_us.load(Ordering::SeqCst))
    }
}

/// A client floods requests and streams and reads nothing. Another client's status still answers at once, the
/// runtime's workers stay free, and once the slow client has taken nothing for the stall limit it is disconnected.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_that_stops_reading_starves_nobody() {
    let limits = Limits {
        write_stall: Duration::from_secs(2),
        ..Limits::default()
    };
    let Some((_d, s, sock)) = daemon("slow", limits) else {
        return;
    };
    let mut slow = Raw::greeted(&sock);
    let flooding = Arc::new(AtomicBool::new(true));
    let mut w = slow.w.try_clone().unwrap();
    let f = Arc::clone(&flooding);
    let flood = std::thread::spawn(move || {
        // Streams that send every 250 ms, then status requests until the daemon stops reading (it has no room left
        // to answer) or hangs up.
        for id in 1..=8u64 {
            let line = format!(
                "{{\"id\":{id},\"op\":\"subscribe\",\"topics\":[\"stats\",\"instances\"],\"interval\":\"250ms\"}}\n"
            );
            if w.write_all(line.as_bytes()).is_err() {
                return;
            }
        }
        let mut id = 100u64;
        while f.load(Ordering::SeqCst) {
            let line = format!("{{\"id\":{id},\"op\":\"status\"}}\n");
            if w.write_all(line.as_bytes()).is_err() {
                return;
            }
            id += 1;
        }
    });
    // Long enough for the slow client's socket buffers and its queue to fill.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let lag = Lag::probe();
    let client = doze_control::Client::socket(&sock);
    let asked = tokio::task::spawn_blocking(move || {
        let c = client.dial().unwrap();
        let mut worst = Duration::ZERO;
        for _ in 0..50 {
            let at = Instant::now();
            assert_eq!(c.snapshot().unwrap().stack, "t");
            worst = worst.max(at.elapsed());
            std::thread::sleep(Duration::from_millis(20));
        }
        worst
    });
    let worst_status = asked.await.unwrap();
    // A worker held by a write to the slow client would show the whole stall limit (2 s) here.
    let worst_lag = lag.worst();
    eprintln!("other client's slowest status: {worst_status:?}; worst runtime lag: {worst_lag:?}");
    assert!(
        worst_status < Duration::from_secs(1),
        "another client waited {worst_status:?} for a status behind a client that reads nothing"
    );
    assert!(
        worst_lag < Duration::from_secs(1),
        "a runtime worker was held for {worst_lag:?} by a client that reads nothing"
    );
    // The stall limit passes with the slow client still reading nothing: it is disconnected (what it was sent before
    // is still there to read, then the end).
    tokio::time::sleep(limits.write_stall + Duration::from_millis(500)).await;
    assert!(
        slow.closed_within(Duration::from_secs(5)),
        "a client that read nothing for the stall limit is still connected"
    );
    flooding.store(false, Ordering::SeqCst);
    let _ = tokio::task::spawn_blocking(move || flood.join()).await;
    // And the daemon serves on.
    let mut after = Raw::greeted(&sock);
    after.say(r#"{"id":1,"op":"status"}"#);
    assert_eq!(after.answer(1)["ok"], true);
    s.stop();
}

/// A connection runs at most `in_flight` requests and streams; past it a request is refused at once, and once one ends
/// there is room again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_runs_at_most_its_cap() {
    let limits = Limits {
        in_flight: 3,
        ..Limits::default()
    };
    let Some((_d, s, sock)) = daemon("inflight", limits) else {
        return;
    };
    let mut c = Raw::greeted(&sock);
    for id in 1..=3 {
        c.say(&format!(
            r#"{{"id":{id},"op":"subscribe","topics":["instances"]}}"#
        ));
        assert_eq!(c.answer(id)["ok"], true);
    }
    c.say(r#"{"id":4,"op":"status"}"#);
    let refused = c.answer(4);
    assert_ne!(refused["ok"], true);
    assert!(
        refused["error"]
            .as_str()
            .unwrap()
            .contains("3 requests and streams running"),
        "{refused}"
    );
    // Another connection has its own room.
    let mut other = Raw::greeted(&sock);
    other.say(r#"{"id":1,"op":"status"}"#);
    assert_eq!(other.answer(1)["ok"], true);
    // Cancelling a stream makes room.
    c.say(r#"{"id":2,"op":"cancel"}"#);
    loop {
        let f = c.hear();
        if f["id"] == 2 && f["end"] == true {
            break;
        }
    }
    let mut room = false;
    for id in 10..60 {
        c.say(&format!(r#"{{"id":{id},"op":"status"}}"#));
        if c.answer(id)["ok"] == true {
            room = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(room, "a cancelled stream still counts against the cap");
    s.stop();
}

/// Past `connections` a client is told why in place of the hello's answer and closed; a slot freed is taken again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_daemon_takes_at_most_its_connections() {
    let limits = Limits {
        connections: 2,
        ..Limits::default()
    };
    let Some((_d, s, sock)) = daemon("conns", limits) else {
        return;
    };
    let a = Raw::greeted(&sock);
    let _b = Raw::greeted(&sock);
    let mut c = Raw::open(&sock);
    c.say(r#"{"hello":{"proto":2}}"#);
    let refused = c.hear();
    assert!(
        refused["error"]
            .as_str()
            .unwrap()
            .contains("serving 2 control connections"),
        "{refused}"
    );
    assert!(c.closed_within(Duration::from_secs(2)));
    drop(a);
    let mut took = false;
    for _ in 0..50 {
        let mut d = Raw::open(&sock);
        d.say(r#"{"hello":{"proto":2}}"#);
        if d.hear().get("hello").is_some() {
            took = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(took, "a closed connection still counts against the cap");
    s.stop();
}

/// Stopping ends the accept at once and takes the socket away; a connection already open is not cut.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopping_ends_the_accept() {
    let Some((_d, s, sock)) = daemon("stop", Limits::default()) else {
        return;
    };
    let mut open = Raw::greeted(&sock);
    let at = Instant::now();
    s.stop();
    assert!(at.elapsed() < Duration::from_millis(100));
    assert!(!sock.exists(), "the socket goes with the server");
    assert!(LocalStream::connect(&sock).is_err());
    open.say(r#"{"id":1,"op":"status"}"#);
    assert_eq!(open.answer(1)["ok"], true);
}
