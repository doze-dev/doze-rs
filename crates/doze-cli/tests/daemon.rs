//! `doze __daemon` started the way `up` starts it (doze-daemon's `ctl`): detached, its socket answering control
//! protocol 2, its pid file in Go's layout, a wake that runs an app of the stack, the stack lock held by one at a
//! time, and a stop that leaves no process, socket or pid file behind.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use doze_daemon::ctl::{self, RunDir, StackLock, Start};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ask(sock: &Path, line: &str) -> String {
    let w = doze_os::local::LocalStream::connect(sock).unwrap();
    w.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let mut r = BufReader::new(w.try_clone().unwrap());
    let mut w = w;
    w.write_all(b"{\"hello\":{\"proto\":2,\"client\":\"test\"}}\n")
        .unwrap();
    let mut hello = String::new();
    r.read_line(&mut hello).unwrap();
    w.write_all(format!("{line}\n").as_bytes()).unwrap();
    let mut answer = String::new();
    r.read_line(&mut answer).unwrap();
    answer
}

#[test]
fn up_starts_the_daemon_and_down_stops_it() {
    // The app's run line goes through `sh -c`, as Go's did; Windows runs commands its own way (W).
    if !Path::new("/bin/sh").exists() {
        return;
    }
    let root = Temp(std::env::temp_dir().join(format!("doze-cli-daemon-{}", std::process::id())));
    let (project, data) = (root.0.join("shop"), root.0.join("data"));
    std::fs::create_dir_all(&project).unwrap();
    let marker = root.0.join("web.ran");
    let hcl = format!(
        "stack = \"shop\"\nhome = \"{}\"\ndata_dir = \"{}\"\n\napp \"web\" {{\n  run = \"echo up > {} && exec sleep 600\"\n}}\n",
        root.0.join("home").display(),
        data.display(),
        marker.display()
    );
    std::fs::write(project.join("doze.hcl"), hcl).unwrap();
    let run = RunDir::of(&data);
    let lock = StackLock::take(&run).unwrap();
    assert!(
        StackLock::try_take(&run).unwrap().is_none(),
        "one holder at a time"
    );
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_doze"));
    let config = project.join("doze.hcl");
    let pid = ctl::start(
        &run,
        &Start {
            exe: &exe,
            config: &config,
            vars: &[],
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    drop(lock);
    assert!(ctl::running(&run));
    let line = std::fs::read_to_string(run.pid_file()).unwrap();
    assert_eq!(
        line,
        format!("{pid}\n"),
        "Go's pid file: the pid on line one"
    );
    assert_eq!(ctl::pid(&run), Some(pid));

    let status = ask(&run.socket(), r#"{"id":1,"op":"status"}"#);
    assert!(
        status.contains("\"ok\":true") && status.contains("\"web\""),
        "{status}"
    );
    let woke = ask(&run.socket(), r#"{"id":2,"op":"wake","db":"web"}"#);
    assert!(woke.contains("\"ok\":true"), "{woke}");
    let t = Instant::now();
    while !marker.exists() && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(marker.exists(), "the app ran");

    let _held = StackLock::take(&run).unwrap();
    assert_eq!(ctl::stop(&run), Ok(true));
    assert!(!doze_os::process::alive(pid), "the daemon is gone");
    assert!(
        !run.socket().exists() && !run.pid_file().exists(),
        "it took its socket and pid file"
    );
    assert_eq!(ctl::stop(&run), Ok(false), "nothing left to stop");
    assert!(
        ctl::tail_log(&run, 5).contains("serving shop"),
        "{}",
        ctl::tail_log(&run, 5)
    );
}
