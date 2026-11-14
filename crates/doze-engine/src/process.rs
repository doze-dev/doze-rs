//! The built-in process engine (`app` and `service` blocks; Go's `engine/process`): a command run directly on the host,
//! supervised, restarted per its policy, ready when its check passes.
//!
//! Run lines are argument lists and never go through a shell (PLAN.md D14); doze-config splits a string into words.
//! A command doze fronts (`port` without `direct`) listens on a private port chosen at every start and told as `$PORT`
//! and `$HOST`; doze holds the declared port. `$DATA` is a folder of its own that survives restarts.

use crate::restart::Restart;
use crate::stack::{Endpoint, PlanCtx, Planned, Sleep};
use doze_module_proto::msg::run::{Plan, Process, Ready, StopHow};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where a fronted command listens, and what it is told as `$HOST`.
pub const PRIVATE_HOST: &str = "127.0.0.1";
/// How long a command may take to be ready unless its block says (Go's `runspec.DefaultReadyTimeout`).
pub const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(60);

/// A `ready = …` check of a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// `tcp()`: its port accepts a connection.
    Tcp,
    /// `http("/path")`: a GET of the path on its port answers 2xx.
    Http(String),
    /// `log("text")`: a line of its output contains the text.
    Log(String),
    /// `exec(…)`: a command exits 0.
    Exec(Vec<String>),
}

/// One `setup`, `on_ready` or `stop` command: as written (what the setup marker hashes, as Go did) and as words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub line: String,
    pub argv: Vec<String>,
}

impl Step {
    /// A step from its words; the line is the words joined by spaces.
    pub fn argv<I: IntoIterator<Item = S>, S: Into<String>>(argv: I) -> Step {
        let argv: Vec<String> = argv.into_iter().map(Into::into).collect();
        Step {
            line: argv.join(" "),
            argv,
        }
    }
}

/// A process block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    /// Program and arguments.
    pub run: Vec<String>,
    /// Where it runs.
    pub dir: PathBuf,
    /// Its `env`, over its `env_file`, over doze's own environment.
    pub env: BTreeMap<String, String>,
    pub env_file: Option<PathBuf>,
    /// The declared port.
    pub port: Option<u16>,
    /// It binds the declared port itself; doze does not front it.
    pub direct: bool,
    pub ready: Option<Probe>,
    pub ready_timeout: Duration,
    pub restart: Restart,
    pub setup: Vec<Step>,
    pub on_ready: Vec<Step>,
    pub stop: Vec<Step>,
}

impl ProcessSpec {
    /// A command run in `dir`.
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(
        run: I,
        dir: impl Into<PathBuf>,
    ) -> ProcessSpec {
        ProcessSpec {
            run: run.into_iter().map(Into::into).collect(),
            dir: dir.into(),
            env: BTreeMap::new(),
            env_file: None,
            port: None,
            direct: false,
            ready: None,
            ready_timeout: DEFAULT_READY_TIMEOUT,
            restart: Restart::default(),
            setup: Vec::new(),
            on_ready: Vec::new(),
            stop: Vec::new(),
        }
    }

    /// Sets one variable.
    pub fn env(mut self, key: &str, value: &str) -> ProcessSpec {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Declares its port.
    pub fn port(mut self, port: u16) -> ProcessSpec {
        self.port = Some(port);
        self
    }

    /// Sets its readiness check.
    pub fn ready(mut self, probe: Probe) -> ProcessSpec {
        self.ready = Some(probe);
        self
    }

    /// Sets its restart policy.
    pub fn restart(mut self, restart: Restart) -> ProcessSpec {
        self.restart = restart;
        self
    }

    /// Whether doze fronts it (Go's `Proxied`): a declared port, not `direct`.
    pub fn proxied(&self) -> bool {
        self.port.is_some() && !self.direct
    }

    /// The port it listens on for this start: the private one when fronted, else the declared one.
    pub fn listen_port(&self, private: Option<u16>) -> Option<u16> {
        if self.proxied() { private } else { self.port }
    }

    /// The variables it is given on top of doze's environment (Go's `envFor`).
    pub fn env_for(&self, ctx: &PlanCtx, private: Option<u16>) -> BTreeMap<String, String> {
        let mut env = self
            .env_file
            .as_deref()
            .map(parse_env_file)
            .unwrap_or_default();
        env.extend(self.env.clone());
        env.insert("DATA".into(), ctx.data_dir.to_string_lossy().into_owned());
        if let (true, Some(p)) = (self.proxied(), private) {
            env.insert("PORT".into(), p.to_string());
            env.insert("HOST".into(), PRIVATE_HOST.into());
        }
        env
    }

    fn ready_check(&self, port: Option<u16>) -> Option<Ready> {
        let probe = match (&self.ready, port) {
            (Some(p), _) => p.clone(),
            (None, Some(_)) => Probe::Tcp,
            (None, None) => return None,
        };
        let at = format!("{PRIVATE_HOST}:{}", port.unwrap_or(0));
        Some(match probe {
            Probe::Tcp => Ready::Tcp(at),
            Probe::Http(path) => Ready::Http(format!("http://{at}{path}")),
            Probe::Log(text) => Ready::LogLine(regex_lite::escape(&text)),
            Probe::Exec(argv) => Ready::Exec(argv),
        })
    }

    /// Checks what Go checked before a start: its folder exists, its `env_file` exists.
    pub fn check(&self, name: &str) -> Result<(), String> {
        if !self.dir.is_dir() {
            return Err(format!(
                "app \"{name}\": dir \"{}\" does not exist",
                self.dir.display()
            ));
        }
        if let Some(f) = &self.env_file
            && !f.exists()
        {
            return Err(format!(
                "{name}: env_file {} does not exist; create it (often by copying .env.example), or remove env_file from the block",
                f.display()
            ));
        }
        if self.run.is_empty() {
            return Err(format!("{name}: run is empty"));
        }
        Ok(())
    }

    /// The plan for one start, `private` being the port picked for it when fronted.
    pub fn planned(&self, ctx: &PlanCtx, private: Option<u16>) -> Result<Planned, String> {
        self.check(&ctx.name)?;
        let (program, args) = self.run.split_first().ok_or("an empty run line")?;
        let port = self.listen_port(private);
        let process = Process {
            name: ctx.name.clone(),
            program: program.clone(),
            args: args.to_vec(),
            env: self.env_for(ctx, private),
            dir: Some(self.dir.to_string_lossy().into_owned()),
            after: Vec::new(),
            ready: self.ready_check(port),
            hooks: Vec::new(),
            // A command is asked with SIGTERM, not the SIGINT a database is written against (Go's `TermFirst`).
            stop: Some(StopHow::Terminate),
        };
        let backend = port
            .map(|p| format!("{PRIVATE_HOST}:{p}"))
            .unwrap_or_default();
        Ok(Planned {
            plan: Plan {
                processes: vec![process],
            },
            endpoint: Endpoint { backend },
            restart: Some(self.restart),
            supervised: true,
            ready_within: self.ready_timeout,
        })
    }
}

/// How the reaper treats an instance (Go: the kind's default, then the spec's `IdleAfter`, then the block's `sleep`):
/// `(always_on, own idle time)`.
pub fn idle_policy(supervised: bool, proxied: bool, sleep: Sleep) -> (bool, Option<Duration>) {
    match sleep {
        Sleep::Never => (true, None),
        Sleep::After(d) => (false, Some(d)),
        // A supervised command stays on, unless doze fronts it: then it sleeps after the stack's time.
        Sleep::Default => (supervised && !proxied, None),
    }
}

/// `KEY=VALUE` lines: blanks and `#` comments skipped, `export ` dropped, one pair of quotes stripped (Go's
/// `parseEnvFile`). A missing file gives nothing.
pub fn parse_env_file(path: &Path) -> BTreeMap<String, String> {
    parse_env(&std::fs::read_to_string(path).unwrap_or_default())
}

/// The text of an env file, read as [`parse_env_file`] does.
// fuzz:parser fuzz_env
pub fn parse_env(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim();
        let unquoted = [('"', '"'), ('\'', '\'')].iter().find_map(|(a, b)| {
            (v.len() >= 2).then_some(())?;
            v.strip_prefix(*a)?.strip_suffix(*b)
        });
        out.insert(k.trim().to_string(), unquoted.unwrap_or(v).to_string());
    }
    out
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
