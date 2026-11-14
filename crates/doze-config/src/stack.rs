//! A loaded config as the engine runs it: every enabled module, app and service as a
//! `doze_engine::Service`, its needs from its dependencies, its sleep, and doze's own variables.
//!
//! Gaps until engine phase E2: tasks are not run (an `after` on a task is not a need yet), and a
//! service's `download` is not fetched (its folder is its `dir`).

use crate::model::{Command, Condition, Config, Instance, Kind, Probe, Sleep, Spec};
use doze_engine::stack::{Need, Planner, Service, Sleep as EngineSleep, Stack, When};
use doze_engine::{Policy, Probe as EngineProbe, ProcessSpec, Restart, Step};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// What the engine needs besides the config.
pub struct Wiring<'a> {
    /// The planner of a module instance (the module pool).
    pub planner: &'a dyn Fn(&Instance) -> Arc<dyn Planner>,
    /// doze's own variables for everything it runs (see [`ca_env`]).
    pub extra_env: BTreeMap<String, String>,
}

impl std::fmt::Debug for Wiring<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wiring")
            .field("extra_env", &self.extra_env)
            .finish()
    }
}

/// The variables that point the usual clients at the doze certificate authority (Go's `caEnv`):
/// Node adds the root to its store; OpenSSL and Python replace theirs, so they get the bundle that
/// still holds the public roots.
pub fn ca_env(root: &Path, bundle: &Path) -> BTreeMap<String, String> {
    let (root, bundle) = (
        root.to_string_lossy().into_owned(),
        bundle.to_string_lossy().into_owned(),
    );
    BTreeMap::from([
        ("DOZE_CA_FILE".to_owned(), root.clone()),
        ("NODE_EXTRA_CA_CERTS".to_owned(), root),
        ("SSL_CERT_FILE".to_owned(), bundle.clone()),
        ("REQUESTS_CA_BUNDLE".to_owned(), bundle),
    ])
}

/// The engine's stack for `cfg`.
pub fn to_stack(cfg: &Config, w: &Wiring<'_>) -> Stack {
    let kept: Vec<&Instance> = cfg
        .instances
        .iter()
        .filter(|i| i.enabled && i.kind() != Kind::Task)
        .collect();
    let names: BTreeSet<&str> = kept.iter().map(|i| i.name.as_str()).collect();
    let services = kept.iter().map(|i| service(i, w, &names)).collect();
    let config_name = Path::new(&cfg.path).file_name().map_or_else(
        || "doze.hcl".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    Stack {
        name: cfg.stack(),
        config_name,
        data_dir: PathBuf::from(&cfg.data_dir),
        idle_timeout: (cfg.defaults.idle_timeout > Duration::ZERO)
            .then_some(cfg.defaults.idle_timeout),
        services,
        extra_env: w.extra_env.clone(),
        // Go's `.doze/state.json` beside doze.hcl (the project's, kept with it); the log limits are E2's defaults
        // until the config's `log_size` is wired (D1 open item).
        state_file: Path::new(&cfg.path)
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".doze")
            .join("state.json"),
        log_limits: Default::default(),
    }
}

fn service(i: &Instance, w: &Wiring<'_>, names: &BTreeSet<&str>) -> Service {
    let (mut s, sleep) = match &i.spec {
        Spec::Command(c) => (Service::process(&i.name, process(c, i.port)), c.sleep),
        _ => (Service::planned(&i.name, (w.planner)(i)), i.sleep),
    };
    s.sleep = engine_sleep(sleep);
    s.needs = i
        .deps
        .iter()
        .filter(|d| names.contains(d.name.as_str()))
        .map(|d| Need {
            name: d.name.clone(),
            when: match d.condition {
                Condition::Healthy => When::Healthy,
                Condition::Started => When::Started,
                Condition::Lazy => When::Lazy,
            },
        })
        .collect();
    s
}

fn engine_sleep(s: Sleep) -> EngineSleep {
    match (s.set, s.never) {
        (false, _) => EngineSleep::Default,
        (true, true) => EngineSleep::Never,
        (true, false) => EngineSleep::After(s.after),
    }
}

/// A command line runs under `sh -c`, as Go runs it.
fn shell(line: &str) -> Vec<String> {
    vec!["sh".into(), "-c".into(), line.into()]
}

fn steps(lines: &[String]) -> Vec<Step> {
    lines
        .iter()
        .map(|l| Step {
            line: l.clone(),
            argv: shell(l),
        })
        .collect()
}

fn probe(p: &Probe) -> EngineProbe {
    match p.kind.as_str() {
        "http" => EngineProbe::Http(p.arg.clone()),
        "log" => EngineProbe::Log(p.arg.clone()),
        "exec" => EngineProbe::Exec(shell(&p.arg)),
        _ => EngineProbe::Tcp,
    }
}

fn process(c: &Command, port: u16) -> ProcessSpec {
    let policy = match c.restart.policy.as_str() {
        "no" => Policy::No,
        "always" => Policy::Always,
        _ => Policy::OnFailure,
    };
    ProcessSpec {
        run: shell(&c.run),
        dir: PathBuf::from(&c.dir),
        env: c.env.clone(),
        env_file: (!c.env_file.is_empty()).then(|| PathBuf::from(&c.env_file)),
        port: (port > 0).then_some(port),
        direct: c.direct,
        ready: c.ready.as_ref().map(probe),
        ready_timeout: c.ready_timeout,
        restart: Restart {
            policy,
            backoff: c.restart.backoff,
            max_retries: u32::try_from(c.restart.max_retries).unwrap_or(u32::MAX),
        },
        setup: steps(&c.setup),
        on_ready: steps(&c.on_ready),
        stop: steps(&c.stop),
        service: c.service,
    }
}
