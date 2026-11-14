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
        // One process may run several (the tests do): each its own folder.
        static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "doze-app-fixture-{}-{run}-{name}",
            std::process::id()
        ));
        Fixture::start_in(name, synth, root, false)
    }

    /// [`Fixture::start`] under `root`; with `real`, the daemon listens on the stack's own control socket, so an app
    /// in another process reading `root/home` as its doze home finds it as it finds a real daemon.
    pub(crate) fn start_in(
        name: &str,
        synth: Option<usize>,
        root: PathBuf,
        real: bool,
    ) -> anyhow::Result<Fixture> {
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
        // `medium-150`, or `medium-150x500` for 500 calls rather than two an app.
        let medium = name.strip_prefix("medium-").and_then(|n| {
            let (n, calls) = n.split_once('x').unwrap_or((n, ""));
            let n = n.parse::<usize>().ok()?;
            Some((n, calls.parse::<usize>().unwrap_or(n * 2)))
        });
        let script = match (synth, medium) {
            (Some(n), _) => synthetic(n),
            (None, Some((n, calls))) => medium_stack(n, calls),
            (None, None) => {
                // `NAME-stale`: NAME's script, from a daemon that goes quiet once it has played (Go's `stale`).
                let file = name.strip_suffix("-stale").unwrap_or(name);
                // The Go hubs' captures, then the app's own (`fixtures/scripts`: `consoles`, built by consoles.ts).
                let path = [scripts(), own().join("scripts")]
                    .into_iter()
                    .map(|d| d.join(format!("{file}.ndjson")))
                    .find(|p| p.exists())
                    .unwrap_or_else(|| scripts().join(format!("{file}.ndjson")));
                let mut s = Script::load(&path).with_context(|| format!("fixture {name}"))?;
                // A verb takes a moment, as a real one does, so a page shows it in flight.
                s.verb_delay = std::time::Duration::from_millis(700);
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
        seed_doze(&home)?;
        let key = StackKey::new(home.join("projects").join(format!("{name}-fixture")));
        std::fs::create_dir_all(key.run_dir()).context("fixture home")?;
        let sock = if real {
            let _ = std::fs::remove_file(key.socket());
            key.socket()
        } else {
            std::fs::write(key.socket(), "").context("fixture socket marker")?;
            temp_socket("app")
        };
        let meters = last_meters(&script);
        let daemon = FixtureDaemon::start(&sock, script).context("fixture daemon")?;
        if !name.ends_with("-stale") {
            tick(daemon.pusher(), meters);
        }
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

impl Fixture {
    /// Has the daemon send a frame to every stream that asked for its topic.
    pub(crate) fn push(&self, f: &doze_control::Frame) {
        if let Some(d) = &self.daemon {
            d.push(f);
        }
    }

    /// What the daemon was asked, in order.
    #[cfg(all(test, feature = "capture"))]
    pub(crate) fn requests(&self) -> Vec<doze_control::Request> {
        self.daemon
            .as_ref()
            .map(FixtureDaemon::requests)
            .unwrap_or_default()
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

/// A stand-in `doze` in the fixture home (`<home>/bin/doze`), which the app's stack actions run as the real CLI: a
/// subprocess, its envelope on stdout, its progress on stderr. As Go's fixture server does, a stack action takes a
/// moment and then fails to start ("this is a fixture"), unless `DOZE_FIXTURE_UP=ok`; down works; `init` and `add`
/// write a small doze.hcl.
fn seed_doze(home: &Path) -> anyhow::Result<()> {
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).context("fixture bin")?;
    doze_os::fs::write_executable(&bin.join("doze"), FAKE_DOZE.as_bytes()).context("fixture doze")
}

const FAKE_DOZE: &str = r#"#!/bin/sh
name=$(basename "$PWD")
case "$1" in
  init) printf 'stack = "%s"\n\napp "web" {\n  run = "npm run dev"\n}\n' "$name" > doze.hcl; echo "wrote doze.hcl"; exit 0;;
  add) printf 'stack = "%s"\n\nmodule %s "%s" { }\n' "$name" "$2" "$2" > doze.hcl; echo "added $2"; exit 0;;
esac
sleep 2
case "$*" in
  *down*) echo '{"schema":"doze/v1","command":"doze down","ok":true}'; exit 0;;
esac
if [ "$DOZE_FIXTURE_UP" = ok ]; then echo '{"schema":"doze/v1","command":"doze up","ok":true}'; exit 0; fi
echo "loading doze.hcl" >&2
echo "starting ${name}…" >&2
echo "{\"schema\":\"doze/v1\",\"command\":\"doze up\",\"ok\":false,\"error\":{\"code\":\"failed\",\"message\":\"this is a fixture; $name cannot really start here\",\"exit\":1}}"
exit 1
"#;

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

/// `medium-N` (or `medium-NxCALLS`): a medium stack of about `n` instances shaped like a real one (doze-map's `synth::medium`: a few teams'
/// services named for the team, a gateway, postgres, redis and kafka, apps nothing calls, a failed migration), with
/// a few in trouble.
fn medium_stack(n: usize, calls: usize) -> Script {
    let ins = doze_map::synth::medium(n, calls, 1);
    let bad = doze_map::synth::medium_trouble(&ins);
    script(&ins, &format!("medium-{n}"), &|i, v| {
        bad.contains(&v.name) || (i % 41 == 40 && v.kind == doze_map::Kind::App)
    })
}

/// A synthetic stack of about `n` instances (doze-map's generator: teams calling down tiers, shared services, tasks),
/// in a spread of states.
fn synthetic(n: usize) -> Script {
    let ins = doze_map::synth::stack(n, n * 5 / 2, 7);
    script(&ins, &format!("synthetic-{n}"), &|i, _| i % 97 == 0)
}

/// A stack's script from instances, in a spread of states; `crashing` says which crash.
fn script(
    ins: &[doze_map::Instance],
    name: &str,
    crashing: &dyn Fn(usize, &doze_map::Instance) -> bool,
) -> Script {
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
            let state = if crashing(i, v) {
                "crashing"
            } else if v.busy {
                // A setup task drawn as a card: running.
                "booting"
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
            stack: name.to_string(),
            dir: "~/code/synthetic".into(),
            instances,
            ..Response::default()
        },
        hold: true,
        ..Script::default()
    }
}

/// The script's last meters frame: what a daemon goes on sending.
fn last_meters(s: &Script) -> Option<doze_control::Frame> {
    s.steps.iter().rev().find_map(|st| match st {
        doze_control::fixture::Step::Frame(f) if f.topic == doze_control::Topic::Stats.as_str() => {
            Some((**f).clone())
        }
        _ => None,
    })
}

/// As Go's fixture server does: the meters every second after the script, as a daemon sends them, so a metered page is
/// never told its picture is old; until the daemon stops.
fn tick(p: doze_control::fixture::Pusher, meters: Option<doze_control::Frame>) {
    let Some(f) = meters else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("fixture-meters".into())
        .spawn(move || {
            while p.push(&f) {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        });
}

/// `$DOZE_APP_FIXTURE_CRASH=NAME@SECONDS[+SECONDS]`: the fixture's NAME crashes that long after launch, as a daemon
/// would say it, and is up again the second time after launch when one is given; so a run shows what the app does
/// about a crash it did not start with (the menu bar's alert, a notification, its withdrawal).
pub(crate) fn crash_later(source: std::rc::Rc<crate::source::Source>, cx: &mut gpui::App) {
    let Some((name, crash, up)) = std::env::var("DOZE_APP_FIXTURE_CRASH").ok().and_then(|v| {
        let (n, s) = v.split_once('@')?;
        let (c, u) = s.split_once('+').map_or((s, None), |(c, u)| (c, Some(u)));
        Some((
            n.to_owned(),
            c.parse::<u64>().ok()?,
            u.and_then(|u| u.parse::<u64>().ok()),
        ))
    }) else {
        return;
    };
    let push = |source: &crate::source::Source, v: doze_control::InstanceView| {
        source.push(&doze_control::Frame {
            topic: "instances".into(),
            instances: vec![v],
            ..doze_control::Frame::default()
        });
    };
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let wait = |s: u64| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(s))
        };
        wait(crash).await;
        let well = source
            .first
            .as_ref()
            .and_then(|k| source.hub.stack(k))
            .and_then(|st| st.instance(&name).cloned());
        let Some(well) = well else { return };
        let mut crashed = well.clone();
        crashed.state = "crashing".into();
        crashed.last_error = "panic: STRIPE_KEY is required".into();
        (crashed.restart_count, crashed.restart_max) = (1, 5);
        push(&source, crashed);
        if let Some(up) = up {
            wait(up.saturating_sub(crash)).await;
            push(&source, well);
        }
    })
    .detach();
}

/// `doze-app fixture-serve NAME ROOT`: the fixture's daemon on its stack's own socket under `ROOT/home`, until killed;
/// an app run with `DOZE_HOME=ROOT/home` finds it as it finds a real daemon, so the app can be weighed without the
/// fixture in its own process.
pub(crate) fn serve(name: &str, root: PathBuf) -> std::process::ExitCode {
    match Fixture::start_in(name, None, root, true) {
        Ok(f) => {
            println!("fixture {name} serving under {}", f.home.display());
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }
        Err(e) => {
            eprintln!("doze-app fixture-serve: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
