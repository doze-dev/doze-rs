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
    /// A hello to a daemon that may be turning this connection away (it may have closed it already: what it said is
    /// still there to read).
    fn hello(&mut self) {
        let _ = self.w.write_all(b"{\"hello\":{\"proto\":2}}\n");
    }
    fn hear(&mut self) -> serde_json::Value {
        self.try_hear()
            .unwrap_or_else(|| panic!("the connection ended"))
    }
    /// The next frame, or `None` at the end of the connection.
    fn try_hear(&mut self) -> Option<serde_json::Value> {
        let mut s = String::new();
        if self.r.read_line(&mut s).ok()? == 0 {
            return None;
        }
        Some(serde_json::from_str(s.trim_end()).unwrap_or_else(|e| panic!("{e}: {s:?}")))
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

/// A client floods requests and streams and reads nothing. Another client's status still answers, the runtime's
/// workers stay free, and the slow client is not cut off: once it reads again, it is answered.
///
/// The bounds are wide on purpose (shared CI runners stall for hundreds of milliseconds under load): the slow client
/// reads nothing for the whole measurement (over a second) and more, so a worker held by a write to it would be held
/// that long and longer, and with both held nothing would answer at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_that_stops_reading_starves_nobody() {
    let Some((_d, s, sock)) = daemon("slow", Limits::default()) else {
        return;
    };
    let slow = Raw::greeted(&sock);
    let later = slow.w.try_clone().unwrap();
    let flooding = Arc::new(AtomicBool::new(true));
    let flood = flood(&slow, &flooding);
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
    let worst_lag = lag.worst();
    eprintln!("other client's slowest status: {worst_status:?}; worst runtime lag: {worst_lag:?}");
    assert!(
        worst_status < Duration::from_secs(2),
        "another client waited {worst_status:?} for a status behind a client that reads nothing"
    );
    assert!(
        worst_lag < Duration::from_secs(2),
        "a runtime worker was held for {worst_lag:?} by a client that reads nothing"
    );
    // The slow client reads again: everything it was owed arrives, then the answer to a new request. It was never
    // disconnected.
    flooding.store(false, Ordering::SeqCst);
    let answered = tokio::task::spawn_blocking(move || reads_again(slow, later, flood))
        .await
        .unwrap();
    assert!(
        answered,
        "the slow client was not answered once it read again"
    );
    s.stop();
}

/// Writes 8 streams (every 250 ms) and then status requests on `c`, from a thread, until `go` turns false or the
/// daemon stops reading (it has no room left to answer).
fn flood(c: &Raw, go: &Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    let mut w = c.w.try_clone().unwrap();
    let f = Arc::clone(go);
    std::thread::spawn(move || {
        // Streams that send every 250 ms, then status requests until the daemon stops reading (it has no room left
        // to answer).
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
    })
}

/// Reads everything owed to `slow` while `flood` ends, then asks one more status on `later` (its write half): whether
/// that is answered.
fn reads_again(mut slow: Raw, mut later: LocalStream, flood: std::thread::JoinHandle<()>) -> bool {
    let (got, answered) = std::sync::mpsc::channel();
    let done = Arc::new(AtomicBool::new(false));
    let d = Arc::clone(&done);
    let drain = std::thread::spawn(move || {
        while !d.load(Ordering::SeqCst) {
            let mut line = String::new();
            match slow.r.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) if line.starts_with("{\"id\":999999,") && line.contains("\"response\"") => {
                    let _ = got.send(true);
                    return;
                }
                Ok(_) => {}
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => break,
            }
        }
        let _ = got.send(false);
    });
    flood.join().unwrap();
    later
        .write_all(b"{\"id\":999999,\"op\":\"status\"}\n")
        .unwrap();
    // Wide: it only has to arrive; a missing answer fails here, a slow one does not.
    let ok = answered.recv_timeout(Duration::from_secs(60)) == Ok(true);
    done.store(true, Ordering::SeqCst);
    drop(later);
    let _ = drain.join();
    ok
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
    // Retried, not timed: the place goes when the cancelled stream's task ends.
    for id in 10..260 {
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
    c.hello();
    let refused = c.hear();
    assert!(
        refused["error"]
            .as_str()
            .unwrap()
            .contains("serving 2 control connections"),
        "{refused}"
    );
    // Closed after at most a moment (the refusal is written with a 200 ms bound); the rest is room for a loaded runner.
    assert!(c.closed_within(Duration::from_secs(10)));
    drop(a);
    let mut took = false;
    // Retried, not timed: the slot goes when the daemon reads the end of the closed connection.
    for _ in 0..250 {
        let mut d = Raw::open(&sock);
        d.hello();
        if d.try_hear().is_some_and(|f| f.get("hello").is_some()) {
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
    // It waits for nothing (no thread to join): even a loaded runner returns from it in well under this.
    assert!(at.elapsed() < Duration::from_secs(1));
    assert!(!sock.exists(), "the socket goes with the server");
    assert!(LocalStream::connect(&sock).is_err());
    open.say(r#"{"id":1,"op":"status"}"#);
    assert_eq!(open.answer(1)["ok"], true);
}

/// Sockets that never say hello hold no slot past the hello deadline: with the cap full of them a real client is
/// turned away, and once the deadline passes it gets in. (100, not the default 128: the test's clients and the daemon
/// share one process, and macOS gives a process 256 descriptors by default; the mechanism is the same.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn silent_sockets_do_not_lock_out_clients() {
    let limits = Limits {
        connections: 100,
        hello: Duration::from_secs(1),
        ..Limits::default()
    };
    let Some((_d, s, sock)) = daemon("silent", limits) else {
        return;
    };
    let silent: Vec<LocalStream> = (0..100)
        .map(|_| LocalStream::connect(&sock).unwrap())
        .collect();
    // Room for the accepts to be counted.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut early = Raw::open(&sock);
    early.hello();
    let refused = early.hear();
    assert!(
        refused["error"]
            .as_str()
            .unwrap_or_default()
            .contains("control connections"),
        "{refused}"
    );
    // Retried for a while: the deadline is 1 s, the rest is room for a loaded runner.
    let mut took = false;
    for _ in 0..100 {
        let mut c = Raw::open(&sock);
        c.hello();
        if c.try_hear().is_some_and(|f| f.get("hello").is_some()) {
            c.say(r#"{"id":1,"op":"status"}"#);
            assert_eq!(c.answer(1)["ok"], true);
            took = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        took,
        "silent sockets still hold every slot past the hello deadline"
    );
    drop(silent);
    s.stop();
}

/// A request that fails lets its place go: with room for one, a failed wake leaves room for the next request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_request_lets_its_place_go() {
    let limits = Limits {
        in_flight: 1,
        ..Limits::default()
    };
    let Some((_d, s, sock)) = daemon("failed", limits) else {
        return;
    };
    let mut c = Raw::greeted(&sock);
    c.say(r#"{"id":1,"op":"wake","db":"nosuch"}"#);
    assert_ne!(c.answer(1)["ok"], true);
    // The place goes just after the answer is queued: a moment of retrying, not a fixed wait.
    let mut ok = false;
    for id in 2..200 {
        c.say(&format!(r#"{{"id":{id},"op":"status"}}"#));
        if c.answer(id)["ok"] == true {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(ok, "a failed request kept its place");
    s.stop();
}

/// A stream id that is open already is refused; once that stream has ended (its `end` read) the id can be used again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_stream_id_is_not_used_twice() {
    let Some((_d, s, sock)) = daemon("dup", Limits::default()) else {
        return;
    };
    let mut c = Raw::greeted(&sock);
    c.say(r#"{"id":5,"op":"subscribe","topics":["instances"]}"#);
    assert_eq!(c.answer(5)["ok"], true);
    c.say(r#"{"id":5,"op":"subscribe","topics":["instances"]}"#);
    let again = c.answer(5);
    assert!(
        again["error"]
            .as_str()
            .unwrap_or_default()
            .contains("already an open stream"),
        "{again}"
    );
    c.say(r#"{"id":5,"op":"cancel"}"#);
    loop {
        let f = c.hear();
        if f["id"] == 5 && f["end"] == true {
            break;
        }
    }
    c.say(r#"{"id":5,"op":"subscribe","topics":["instances"]}"#);
    assert_eq!(
        c.answer(5)["ok"],
        true,
        "the id is free once its stream has ended"
    );
    s.stop();
}
