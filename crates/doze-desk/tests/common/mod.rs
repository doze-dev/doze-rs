//! What the hub's tests share: a doze home laid out as the Go app finds stacks, fixture daemons on real sockets
//! playing the Go fixture hubs, and waiting.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use doze_control::fixture::{FixtureDaemon, Script, temp_socket};
use doze_control::{Backoff, Client, Timestamp};
use doze_desk::{Clock, Hub, Options, StackKey, StackView};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// When every Go golden is computed (Go's view tests' `epoch`).
pub(crate) fn epoch() -> SystemTime {
    serde_json::from_str::<Timestamp>("\"2026-10-11T14:04:40Z\"")
        .unwrap()
        .to_system_time()
        .unwrap()
}

/// A Go fixture hub's script, as `doze-control` captured it from Go.
pub(crate) fn script(hub: &str) -> Script {
    Script::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../doze-control/testdata/go/scripts/{hub}.ndjson")),
    )
    .unwrap()
}

/// A fixture daemon playing `s`; `None` (the test is skipped) where the system has no local control socket yet.
pub(crate) fn daemon(tag: &str, s: Script) -> Option<FixtureDaemon> {
    daemon_at(&temp_socket(tag), s)
}

pub(crate) fn daemon_at(path: &Path, s: Script) -> Option<FixtureDaemon> {
    match FixtureDaemon::start(path, s) {
        Ok(d) => Some(d),
        Err(e) if e.kind() == io::ErrorKind::Unsupported => {
            eprintln!("skipped: {e}");
            None
        }
        Err(e) => panic!("{}: {e}", path.display()),
    }
}

/// A doze home. Each stack has a plain file where its control socket goes (that is all discovery looks at) and is
/// dialled at a fixture daemon's own socket, as Go's hub tests hand fakes out through `Dial`.
pub(crate) struct Home {
    pub(crate) dir: tempfile::TempDir,
    routes: Arc<Mutex<HashMap<StackKey, PathBuf>>>,
}

impl Home {
    pub(crate) fn new() -> Home {
        Home {
            dir: tempfile::tempdir().unwrap(),
            routes: Arc::default(),
        }
    }

    /// The key of the stack in folder `slug`.
    pub(crate) fn key(&self, slug: &str) -> StackKey {
        StackKey::new(self.dir.path().join("projects").join(slug))
    }

    /// A running stack in folder `slug`, dialled at `daemon`.
    pub(crate) fn stack(&self, slug: &str, daemon: &FixtureDaemon) -> StackKey {
        let key = self.key(slug);
        std::fs::create_dir_all(key.run_dir()).unwrap();
        std::fs::write(key.socket(), "").unwrap();
        self.route(&key, daemon.path());
        key
    }

    pub(crate) fn route(&self, key: &StackKey, sock: &Path) {
        self.routes
            .lock()
            .unwrap()
            .insert(key.clone(), sock.to_path_buf());
    }

    /// Takes the stack's socket away: its daemon exited and cleaned up.
    pub(crate) fn remove_socket(&self, key: &StackKey) {
        std::fs::remove_file(key.socket()).unwrap();
    }

    /// Puts it back.
    pub(crate) fn put_socket(&self, key: &StackKey) {
        std::fs::create_dir_all(key.run_dir()).unwrap();
        std::fs::write(key.socket(), "").unwrap();
    }

    /// Writes the project record of the stack in folder `slug`.
    pub(crate) fn record(&self, slug: &str, json: &str) -> StackKey {
        let key = self.key(slug);
        std::fs::create_dir_all(key.dir()).unwrap();
        std::fs::write(key.record(), json).unwrap();
        key
    }

    /// A hub's options on this home: quick to retry, at the Go goldens' moment.
    pub(crate) fn options(&self) -> Options {
        let routes = Arc::clone(&self.routes);
        let mut o = Options::new(self.dir.path());
        o.dial = Arc::new(move |key, sock| {
            let to = routes
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .unwrap_or_else(|| sock.to_path_buf());
            Client::socket(to).named("test")
        });
        o.every = Duration::from_millis(20);
        o.clock = Clock::fixed(epoch());
        o.backoff = Backoff {
            first: Duration::from_millis(40),
            max: Duration::from_millis(200),
        };
        o
    }
}

/// Waits (up to five seconds) until `ok` holds of the stack `key`; returns it.
pub(crate) fn until(
    hub: &Hub,
    key: &StackKey,
    what: &str,
    ok: impl Fn(&StackView) -> bool,
) -> StackView {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(s) = hub.stack(key).filter(|s| ok(s)) {
            return s;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting until {what}; stack: {:#?}",
            hub.stack(key).map(|s| (s.link, s.instances.len()))
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Waits (up to five seconds) until `ok`.
pub(crate) fn wait(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ok() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A started hub following the Go fixture hub `name` in folder `p-1`, live.
pub(crate) fn following(name: &str) -> Option<(Home, FixtureDaemon, Hub, StackKey)> {
    let mut s = script(name);
    // A real daemon holds every subscription open; the empty capture does not say so (Go's fake had nothing to
    // stream), and a stream that ends is a daemon that went away.
    s.hold = true;
    let n = s.status.instances.len();
    let d = daemon(name, s)?;
    let home = Home::new();
    let key = home.stack("p-1", &d);
    let hub = Hub::start(home.options());
    until(&hub, &key, "live, described, every instance in", |s| {
        s.link.is_live() && !s.name.is_empty() && s.instances.len() == n
    });
    Some((home, d, hub, key))
}
