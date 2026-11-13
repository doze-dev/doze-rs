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

/// A fixture daemon and the home that points at it. Dropped, the daemon stops and the home is removed.
pub(crate) struct Fixture {
    pub(crate) key: StackKey,
    home: PathBuf,
    daemon: Option<FixtureDaemon>,
}

impl Fixture {
    /// Starts `name`'s script, or a synthetic stack of `synth` instances.
    pub(crate) fn start(name: &str, synth: Option<usize>) -> anyhow::Result<Fixture> {
        let script = match synth {
            Some(n) => synthetic(n),
            None => {
                let path = scripts().join(format!("{name}.ndjson"));
                let mut s = Script::load(&path).with_context(|| format!("fixture {name}"))?;
                // Keep streams open, as a daemon does: the page stays live.
                s.hold = true;
                s
            }
        };
        let home =
            std::env::temp_dir().join(format!("doze-app-fixture-{}-{name}", std::process::id()));
        let key = StackKey::new(home.join("projects").join(format!("{name}-fixture")));
        std::fs::create_dir_all(key.run_dir()).context("fixture home")?;
        std::fs::write(key.socket(), "").context("fixture socket marker")?;
        let daemon = FixtureDaemon::start(&temp_socket("app"), script).context("fixture daemon")?;
        Ok(Fixture {
            key,
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
        o.dial = Arc::new(move |_, _| Client::socket(sock.clone()).named("app"));
        o
    }
}

impl Fixture {
    /// Has the daemon send a frame to every stream that asked for its topic.
    pub(crate) fn push(&self, f: &doze_control::Frame) {
        if let Some(d) = &self.daemon {
            d.push(f);
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(d) = self.daemon.take() {
            d.stop();
        }
        let _ = std::fs::remove_dir_all(&self.home);
    }
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

/// A clock that starts at the moment the Go fixtures were recorded at (their goldens' epoch, 2026-10-11T14:04:40Z) and
/// runs from there, so a fixture's ages read as the goldens do while a countdown still moves.
#[cfg(feature = "capture")]
pub(crate) fn recorded_clock() -> doze_desk::Clock {
    let epoch = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_791_727_480);
    let start = std::time::Instant::now();
    doze_desk::Clock::from_fn(move || epoch + start.elapsed())
}
