//! A stack for development and screenshots: doze-control's fixture daemon playing a Go fixture hub's script (or a
//! synthetic stack) on a real socket, under a throwaway doze home laid out the way the hub finds stacks. Nothing here
//! touches the real doze home.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use doze_control::fixture::{FixtureDaemon, Script, temp_socket};
use doze_control::{Client, InstanceView, Response};
use doze_desk::{Options, StackKey};

/// The Go fixture hubs' scripts (`crates/doze-control/testdata/go/scripts`), or `$DOZE_FIXTURES`.
fn scripts() -> PathBuf {
    std::env::var_os("DOZE_FIXTURES").map_or_else(
        || {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../crates/doze-control/testdata/go/scripts")
        },
        PathBuf::from,
    )
}

/// When the Go fixture hubs were captured (Go's view tests' `epoch`): a fixture's clock starts here and runs, so "up 14m" and
/// "last up 3d ago" read as they did in Go's goldens whenever the shot is taken.
fn epoch() -> std::time::SystemTime {
    std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_791_727_480)
}

/// The app's own fixture folders (`fixtures/`): the stopped stack every fixture home carries.
fn own() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// A fixture daemon and the home that points at it. Dropped, the daemon stops and the home is removed.
pub(crate) struct Fixture {
    /// The running stack; `None` for `none`, a machine with no stacks.
    pub(crate) key: Option<StackKey>,
    root: PathBuf,
    home: PathBuf,
    daemon: Option<FixtureDaemon>,
}

impl Fixture {
    /// Starts `name`'s script, or a synthetic stack of `synth` instances. Beside it, as the Go fixture homes do, a
    /// stopped stack (`acme`, up three days ago with api, web and db). `none` is a machine with no stacks at all.
    pub(crate) fn start(name: &str, synth: Option<usize>) -> anyhow::Result<Fixture> {
        let root =
            std::env::temp_dir().join(format!("doze-app-fixture-{}-{name}", std::process::id()));
        let (home, work) = (root.join("home"), root.join("work"));
        std::fs::create_dir_all(&home).context("fixture home")?;
        if name == "none" && synth.is_none() {
            return Ok(Fixture {
                key: None,
                root,
                home,
                daemon: None,
            });
        }
        let script = match synth {
            Some(n) => synthetic(n),
            None => {
                let path = scripts().join(format!("{name}.ndjson"));
                let mut s = Script::load(&path).with_context(|| format!("fixture {name}"))?;
                // Keep streams open, as a daemon does: the page stays live.
                s.hold = true;
                if name.starts_with("larder") {
                    // The Grid quotes the file: put larder's doze.hcl where the stack says it runs from.
                    s.status.dir = seed_larder(&work, s.status.loaded.to_system_time())?;
                }
                s
            }
        };
        seed_stopped(&home, &work)?;
        let key = StackKey::new(home.join("projects").join(format!("{name}-fixture")));
        std::fs::create_dir_all(key.run_dir()).context("fixture home")?;
        std::fs::write(key.socket(), "").context("fixture socket marker")?;
        let daemon = FixtureDaemon::start(&temp_socket("app"), script).context("fixture daemon")?;
        Ok(Fixture {
            key: Some(key),
            root,
            home,
            daemon: Some(daemon),
        })
    }

    /// A hub's options on the fixture home, dialling the fixture daemon for its stack.
    pub(crate) fn options(&self) -> Options {
        let sock = self
            .daemon
            .as_ref()
            .map(|d| d.path().to_path_buf())
            .unwrap_or_default();
        let mut o = Options::new(self.home.clone());
        let start = std::time::Instant::now();
        o.clock = doze_desk::Clock::from_fn(move || epoch() + start.elapsed());
        o.dial = Arc::new(move |_, _| Client::socket(sock.clone()).named("app"));
        o
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(d) = self.daemon.take() {
            d.stop();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// acme: its folder in `work` with its doze.hcl (and what `doze --json status` says of it), and in `home` the project
/// record that says it was up three days ago with api, web and db (Go devserve's `seedStopped`).
fn seed_stopped(home: &Path, work: &Path) -> anyhow::Result<()> {
    let dir = work.join("acme");
    std::fs::create_dir_all(&dir).context("acme")?;
    for f in ["doze.hcl", "status.json"] {
        std::fs::copy(own().join("acme").join(f), dir.join(f)).context("acme files")?;
    }
    let then = epoch() - std::time::Duration::from_secs(3 * 24 * 3600);
    let at = |t| {
        doze_control::Timestamp::from_system_time(t)
            .as_str()
            .to_owned()
    };
    let record = serde_json::json!({
        "schema": 1,
        "config": dir.join("doze.hcl"),
        "name": "acme",
        "last_up": at(then),
        "last_down": at(then + std::time::Duration::from_secs(6 * 3600)),
        "last_set": ["api", "web", "db"],
    });
    let projects = home.join("projects").join("acme");
    std::fs::create_dir_all(&projects).context("acme record")?;
    std::fs::write(projects.join("project.json"), record.to_string()).context("acme record")?;
    Ok(())
}

/// larder's doze.hcl in `work`; its folder.
fn seed_larder(work: &Path, loaded: Option<std::time::SystemTime>) -> anyhow::Result<String> {
    let dir = work.join("larder");
    std::fs::create_dir_all(&dir).context("larder")?;
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/doze-desk/testdata/go/hcl/larder.hcl");
    std::fs::copy(src, dir.join("doze.hcl")).context("larder's doze.hcl")?;
    // Saved a minute before the stack read it, as Go's fixture does: not "doze.hcl changed".
    if let Some(t) = loaded.and_then(|l| l.checked_sub(std::time::Duration::from_secs(60))) {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(dir.join("doze.hcl"))?;
        f.set_modified(t)?;
    }
    Ok(dir.display().to_string())
}

/// A synthetic stack of about `n` instances (doze-map's generator: teams calling down tiers, shared services, tasks),
/// in a spread of states.
fn synthetic(n: usize) -> Script {
    let ins = doze_map::synth::stack(n, n * 5 / 2, 7);
    let states = ["active", "active", "idle", "reaped", "booting"];
    let instances = ins
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let kind = match v.kind {
                doze_map::Kind::App => "app",
                doze_map::Kind::Task => "task",
                doze_map::Kind::Service => "service",
                _ => "module",
            };
            let state = if i % 97 == 0 {
                "crashing"
            } else {
                states.get(i % states.len()).copied().unwrap_or("reaped")
            };
            let awake = matches!(state, "active" | "idle");
            InstanceView {
                name: v.name.clone(),
                kind: kind.into(),
                needs: v.needs.clone(),
                run: v.run.clone(),
                engine: v.engine.clone(),
                state: state.into(),
                ram: if awake {
                    i64::try_from((6 + (i * 37) % 58) << 20).unwrap_or(0)
                } else {
                    0
                },
                cpu: if awake { 0.01 } else { 0.0 },
                ..InstanceView::default()
            }
        })
        .collect();
    Script {
        status: Response {
            ok: true,
            stack: format!("synthetic-{n}"),
            dir: "~/code/synthetic".into(),
            instances,
            ..Response::default()
        },
        hold: true,
        ..Script::default()
    }
}
