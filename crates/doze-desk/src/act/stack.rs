//! A whole stack's actions (Go `serve.Stack`): up as last time, up everything, up a chosen set, down, restart. A
//! stopped stack has no daemon to ask, so the CLI does them, in the stack's folder: `doze --json -y up --last` and
//! the rest, its machine contract (one envelope on stdout; progress on stderr; the exit code). Restart is down, then
//! up as last time, one after the other: two separate actions would race. Removing a stack is [`super::remove`].
//!
//! The run blocks: the app calls it off the UI thread, and marks the stack busy meanwhile ([`Busy`]).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use super::failure::{Failure, failure_of};

/// What to do to a stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackAction {
    /// Up as last time (the switch).
    UpLast,
    /// Up everything declared.
    UpAll,
    /// Up these, with what they need (the start sheet).
    Up(Vec<String>),
    Down,
    /// Down, then up as last time.
    Restart,
    /// Stop it, and stop listing it; every file stays.
    Remove,
}

/// What a stack is busy doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Busy {
    Starting,
    Stopping,
    Restarting,
    Removing,
}

impl Busy {
    /// `starting`, as the row and the banner say it ("starting…").
    pub fn word(self) -> &'static str {
        match self {
            Busy::Starting => "starting",
            Busy::Stopping => "stopping",
            Busy::Restarting => "restarting",
            Busy::Removing => "removing",
        }
    }

    /// The banner's second line.
    pub fn detail(self) -> &'static str {
        match self {
            Busy::Stopping => "Every service stops; data stays.",
            Busy::Removing => "Its services stop; its folder and data stay on disk.",
            Busy::Starting | Busy::Restarting => {
                "Services start in the order they need each other; this page follows as they come up."
            }
        }
    }

    /// Whether the switch shows on while it is busy (everything but stopping is heading up).
    pub fn heading_up(self) -> bool {
        matches!(self, Busy::Starting | Busy::Restarting)
    }
}

impl StackAction {
    /// The action Go's pages name (`up-last`, `up-all`, `up` with `chosen`, `down`, `restart`, `remove`/`forget`).
    pub fn parse(action: &str, chosen: &[String]) -> Option<StackAction> {
        Some(match action {
            "up-last" => StackAction::UpLast,
            "up-all" => StackAction::UpAll,
            "up" => StackAction::Up(chosen.to_vec()),
            "down" => StackAction::Down,
            "restart" => StackAction::Restart,
            "remove" | "forget" => StackAction::Remove,
            _ => return None,
        })
    }

    /// Its name, as [`StackAction::parse`] takes it.
    pub fn name(&self) -> &'static str {
        match self {
            StackAction::UpLast => "up-last",
            StackAction::UpAll => "up-all",
            StackAction::Up(_) => "up",
            StackAction::Down => "down",
            StackAction::Restart => "restart",
            StackAction::Remove => "remove",
        }
    }

    /// What the stack is while it runs.
    pub fn busy(&self) -> Busy {
        match self {
            StackAction::UpLast | StackAction::UpAll | StackAction::Up(_) => Busy::Starting,
            StackAction::Down => Busy::Stopping,
            StackAction::Restart => Busy::Restarting,
            StackAction::Remove => Busy::Removing,
        }
    }

    /// What is said as it starts: "bringing shop up as last time…".
    pub fn said(&self, stack: &str) -> String {
        match self {
            StackAction::UpLast => format!("bringing {stack} up as last time…"),
            StackAction::UpAll => format!("bringing all of {stack} up…"),
            StackAction::Up(names) => format!("bringing up {}…", names.join(", ")),
            StackAction::Down => format!("stopping {stack}…"),
            StackAction::Restart => format!("restarting {stack} on the new doze.hcl…"),
            StackAction::Remove => format!("removing {stack} from doze…"),
        }
    }

    /// The headline when it does not work: "Couldn't start shop".
    pub fn failed(&self, stack: &str) -> String {
        let verb = match self.busy() {
            Busy::Starting => "start",
            Busy::Stopping => "stop",
            Busy::Restarting => "restart",
            Busy::Removing => return format!("Removing {stack} failed"),
        };
        format!("Couldn't {verb} {stack}")
    }

    /// The CLI runs it takes, in order: each a command line after `doze`.
    pub fn runs(&self) -> Vec<Vec<String>> {
        let line = |a: &[&str]| -> Vec<String> {
            ["--json", "-y"]
                .iter()
                .chain(a)
                .map(|s| (*s).to_owned())
                .collect()
        };
        match self {
            StackAction::UpLast => vec![line(&["up", "--last"])],
            StackAction::UpAll => vec![line(&["up", "--all"])],
            StackAction::Up(names) => {
                let mut l = line(&["up"]);
                l.extend(names.iter().cloned());
                vec![l]
            }
            StackAction::Down => vec![line(&["down"])],
            StackAction::Restart => vec![line(&["down"]), line(&["up", "--last"])],
            StackAction::Remove => Vec::new(),
        }
    }
}

/// The doze CLI, run for a stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    /// The `doze` to run.
    pub program: PathBuf,
    /// The doze home it works in: the one the app shows (a fixture's own, never the real one by accident).
    pub home: PathBuf,
}

/// What one CLI run ended with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ran {
    pub ok: bool,
    /// What it printed on stderr (its progress, its warnings), then its error, as a person would have read it.
    pub printed: String,
}

#[derive(Debug, Default, Deserialize)]
struct Envelope {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    error: Option<EnvelopeError>,
}

#[derive(Debug, Default, Deserialize)]
struct EnvelopeError {
    #[serde(default)]
    message: String,
}

impl Cli {
    /// Runs `doze <args>` in `dir` and waits.
    pub fn run(&self, dir: &Path, args: &[String]) -> Ran {
        let out = Command::new(&self.program)
            .args(args)
            .current_dir(dir)
            .env("NO_COLOR", "1")
            .env("DOZE_HOME", &self.home)
            .env_remove("DOZE_APP_CHILD")
            .output();
        match out {
            Ok(o) => read_ran(
                o.status.success(),
                &String::from_utf8_lossy(&o.stdout),
                &String::from_utf8_lossy(&o.stderr),
            ),
            Err(e) => Ran {
                ok: false,
                printed: format!("could not run {}: {e}", self.program.display()),
            },
        }
    }

    /// Does `action` to the stack in `dir` named `stack`: each of its runs in turn, stopping at the first that fails.
    pub fn stack(&self, action: &StackAction, stack: &str, dir: &Path) -> Result<(), Failure> {
        for args in action.runs() {
            let ran = self.run(dir, &args);
            if !ran.ok {
                return Err(failure_of(&action.failed(stack), dir, &ran.printed));
            }
        }
        Ok(())
    }
}

// fuzz:parser fuzz_envelope
/// One run's result from its exit, its stdout (the `--json` envelope) and its stderr. The envelope's `ok` must agree
/// with the exit; an older CLI with no envelope is read by its exit alone, its output as printed.
pub fn read_ran(exit_ok: bool, stdout: &str, stderr: &str) -> Ran {
    let env: Option<Envelope> = serde_json::from_str(stdout.trim()).ok();
    let mut printed = stderr.trim_end().to_owned();
    let message = env
        .as_ref()
        .and_then(|e| e.error.as_ref())
        .map(|e| e.message.trim().to_owned())
        .filter(|m| !m.is_empty());
    match (message, &env) {
        (Some(m), _) => {
            if !printed.contains(&m) {
                if !printed.is_empty() {
                    printed.push('\n');
                }
                printed.push_str("Error: ");
                printed.push_str(&m);
            }
        }
        (None, None) if !stdout.trim().is_empty() => {
            if !printed.is_empty() {
                printed.push('\n');
            }
            printed.push_str(stdout.trim_end());
        }
        _ => {}
    }
    Ran {
        ok: exit_ok && env.as_ref().is_none_or(|e| e.ok),
        printed,
    }
}

#[cfg(test)]
mod tests {
    use super::{Busy, StackAction, read_ran};

    #[test]
    fn each_action_runs_the_cli_as_go_did() {
        assert_eq!(
            StackAction::UpLast.runs(),
            vec![vec!["--json", "-y", "up", "--last"]]
        );
        let up = StackAction::parse("up", &["web".into(), "worker".into()]);
        assert_eq!(
            up.as_ref().map(StackAction::runs),
            Some(vec![
                vec!["--json", "-y", "up", "web", "worker"]
                    .into_iter()
                    .map(String::from)
                    .collect()
            ])
        );
        let r = StackAction::Restart.runs();
        assert_eq!(r.len(), 2, "down, then up as last time");
        assert_eq!(StackAction::Restart.busy(), Busy::Restarting);
        assert_eq!(StackAction::Down.said("shop"), "stopping shop…");
        assert_eq!(StackAction::UpAll.failed("shop"), "Couldn't start shop");
        assert!(StackAction::parse("nope", &[]).is_none());
        assert!(Busy::Starting.heading_up() && !Busy::Stopping.heading_up());
    }

    #[test]
    fn a_run_is_read_from_its_envelope_and_its_exit() {
        let ok = read_ran(true, r#"{"schema":"doze/v1","ok":true}"#, "starting api…");
        assert!(ok.ok);
        let bad = read_ran(
            false,
            r#"{"schema":"doze/v1","ok":false,"error":{"code":"config_invalid","message":"doze.hcl:3,1-4: Unsupported block type","exit":5}}"#,
            "loading doze.hcl",
        );
        assert!(!bad.ok);
        assert_eq!(
            bad.printed,
            "loading doze.hcl\nError: doze.hcl:3,1-4: Unsupported block type"
        );
        let old = read_ran(false, "doze: no such flag --json\n", "");
        assert_eq!(old.printed, "doze: no such flag --json");
        let disagree = read_ran(true, r#"{"ok":false}"#, "");
        assert!(!disagree.ok, "an envelope that says it failed failed");
    }
}
