//! The app's stack actions against a REAL daemon (the owner's rule: fixture playback hid that nothing reacts). The
//! built `doze-app` is the CLI (`doze-app up` …) and the daemon (`doze-app __daemon`), on a throwaway `DOZE_HOME`, never
//! `~/.doze`; the stack is two apps that are `nc` listening on their ports. The app's own hub follows it, and what
//! Needs you, the Library and the menu bar would say is read from it every 25 ms while the stack goes up and down.
//!
//! - ten ups and downs, run as the app runs them (`Hub::set_acting` around each), and three more without it (the
//!   2 s rule alone): "the daemon is not answering" never appears;
//! - Sleep everything (`sleep` with no target) leaves the daemon up with every service asleep, and a connection wakes
//!   the one it is for;
//! - a daemon killed outright is still flagged at once (its stack "stopped without being asked").
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use doze_control::Verb;
use doze_desk::act::{Cli, StackAction};
use doze_desk::attention::{StackFacts, stack_items};
use doze_desk::{Hub, Options, StackKey, StackView};

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn have(program: &str) -> bool {
    ["/usr/bin", "/bin"]
        .iter()
        .any(|d| Path::new(d).join(program).exists())
}

struct Stack {
    root: PathBuf,
    home: PathBuf,
    dir: PathBuf,
    cli: Cli,
    ports: (u16, u16),
}

impl Stack {
    fn new() -> Stack {
        // A control socket's path must fit a sockaddr (104 bytes on macOS): the system's temp folder is long there,
        // so a short root under /tmp when there is one.
        let tmp = std::env::temp_dir();
        let base = if tmp.as_os_str().len() > 24 && Path::new("/tmp").is_dir() {
            PathBuf::from("/tmp")
        } else {
            tmp
        };
        let root = base.join(format!("dz-rd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (home, dir) = (root.join("home"), root.join("shop"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        // As the daemon says it (/tmp is /private/tmp on macOS).
        let dir = std::fs::canonicalize(&dir).unwrap();
        let ports = (free_port(), free_port());
        let hcl = format!(
            "stack = \"shop\"\n\napp \"web\" {{\n  run  = \"exec nc -lk $HOST $PORT\"\n  port = {}\n}}\n\napp \"api\" {{\n  run  = \"exec nc -lk $HOST $PORT\"\n  port = {}\n}}\n",
            ports.0, ports.1
        );
        std::fs::write(dir.join("doze.hcl"), hcl).unwrap();
        let cli = Cli {
            program: PathBuf::from(env!("CARGO_BIN_EXE_doze-app")),
            home: home.clone(),
        };
        Stack {
            root,
            home,
            dir,
            cli,
            ports,
        }
    }

    /// Runs a stack action as `crate::act` does: acting around it when `acting`, the hub looked at again after.
    fn act(&self, hub: &Hub, action: &StackAction, acting: bool) {
        let key = self.key(hub);
        if acting && let Some(k) = &key {
            hub.set_acting(k, true);
        }
        let done = self.cli.stack(action, "shop", &self.dir);
        if acting && let Some(k) = &key {
            hub.set_acting(k, false);
        }
        hub.refresh();
        if let Err(f) = done {
            panic!("{action:?} failed: {} {:?}", f.problem, f.lines);
        }
    }

    fn key(&self, hub: &Hub) -> Option<StackKey> {
        hub.stacks()
            .into_iter()
            .find(|s| Path::new(&s.dir) == self.dir.as_path())
            .map(|s| s.key)
    }

    fn view(&self, hub: &Hub) -> Option<StackView> {
        let k = self.key(hub)?;
        hub.stack(&k)
    }

    /// The daemon's pid, from its record.
    fn pid(&self) -> Option<u32> {
        doze_projects::running(&self.home)
            .into_iter()
            .find_map(|r| r.pid)
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        let _ = self.cli.stack(&StackAction::Down, "shop", &self.dir);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Watches the stack every 25 ms: every "not answering" anything would say (Needs you's item, the Library's pill).
fn watch(hub: Hub, dir: PathBuf, stop: Arc<AtomicBool>, seen: Arc<Mutex<Vec<String>>>) {
    while !stop.load(Ordering::SeqCst) {
        let now = hub.now();
        for st in hub
            .stacks()
            .into_iter()
            .filter(|s| Path::new(&s.dir) == dir.as_path())
        {
            for it in stack_items(&st, &StackFacts::default(), now) {
                if it.kind == "daemon" {
                    seen.lock()
                        .unwrap()
                        .push(format!("{}: {}", it.title, it.detail));
                }
            }
            if st.daemon_lost(now) {
                seen.lock()
                    .unwrap()
                    .push(format!("lost: {:?} {}", st.link.conn, st.link.error));
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn on_and_off_never_says_not_answering_and_sleep_all_keeps_the_daemon() {
    if !cfg!(unix) || !have("nc") || !have("sh") {
        eprintln!("skipped: needs a unix shell and nc");
        return;
    }
    let s = Stack::new();
    let hub = Hub::start(Options::new(&s.home));
    let (stop, seen) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(Vec::new())),
    );
    let watcher = {
        let (hub, dir, stop, seen) = (hub.clone(), s.dir.clone(), stop.clone(), seen.clone());
        std::thread::spawn(move || watch(hub, dir, stop, seen))
    };

    up_and_down(&s, &hub, &seen);
    let pid = sleep_all(&s, &hub);
    crash(&s, &hub, pid);
    stop.store(true, Ordering::SeqCst);
    watcher.join().unwrap();
    let said = seen.lock().unwrap().clone();
    assert!(said.is_empty(), "said around Sleep everything: {said:?}");
}

/// Ten ups and downs as the app's switch does them, three more without telling the hub: nothing says not answering.
fn up_and_down(s: &Stack, hub: &Hub, seen: &Mutex<Vec<String>>) {
    for (i, acting) in (0..13).map(|i| (i, i < 10)) {
        let up = if i == 0 {
            StackAction::UpAll
        } else {
            StackAction::UpLast
        };
        s.act(hub, &up, acting);
        until("running", 20, || s.view(hub).is_some_and(|v| v.running));
        s.act(hub, &StackAction::Down, acting);
        until("stopped", 20, || s.view(hub).is_some_and(|v| !v.running));
    }
    // A moment more, for a loss counted late.
    std::thread::sleep(doze_desk::LOST_AFTER + Duration::from_millis(500));
    let said = seen.lock().unwrap().clone();
    assert!(said.is_empty(), "said while going up and down: {said:?}");
}

/// Sleep everything: the daemon stays up, every service asleep; a connection wakes the one it is for. Its pid.
fn sleep_all(s: &Stack, hub: &Hub) -> u32 {
    s.act(hub, &StackAction::UpLast, true);
    until("live", 20, || {
        s.view(hub).is_some_and(|v| v.running && v.link.is_live())
    });
    let key = s.key(hub).unwrap();
    hub.verb(&key, Verb::Wake, "web").unwrap();
    until("web awake", 20, || {
        s.view(hub)
            .is_some_and(|v| v.instance("web").is_some_and(|i| i.pid > 0))
    });
    let pid = s.pid().expect("the daemon's pid");
    hub.verb(&key, Verb::Sleep, "").unwrap();
    until("all asleep", 20, || {
        s.view(hub).is_some_and(|v| v.all_asleep())
    });
    hub.refresh();
    let v = s.view(hub).unwrap();
    assert!(
        v.running && v.link.is_live(),
        "the daemon stays up: {:?}",
        v.link
    );
    assert_eq!(s.pid(), Some(pid), "the same daemon");
    assert!(doze_os::process::alive(pid));
    let _c = std::net::TcpStream::connect(("127.0.0.1", s.ports.0)).unwrap();
    until("web woken by a connection", 20, || {
        s.view(hub).is_some_and(|v| !v.all_asleep())
    });

    pid
}

/// A daemon killed outright is flagged at once: its stack stopped without being asked (once up for 30 s, the grace a
/// start has). Its apps are its children, stopped by their pids too.
fn crash(s: &Stack, hub: &Hub, pid: u32) {
    std::thread::sleep(Duration::from_secs(31));
    let children = std::process::Command::new("pgrep")
        .args(["-P", &pid.to_string()])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status();
    for c in children.split_whitespace() {
        let _ = std::process::Command::new("kill").args(["-9", c]).status();
    }
    let t = Instant::now();
    until("the crash flagged", 5, || {
        hub.refresh();
        s.view(hub).is_some_and(|v| {
            stack_items(&v, &StackFacts::default(), hub.now())
                .iter()
                .any(|i| i.kind == "ended" && i.red)
        })
    });
    eprintln!("crash flagged after {:?}", t.elapsed());
}
