//! The machine's privileged actions from the app, as data: what a job will do ([`Planned`], shown before any password
//! is asked for) and running it through [`Hands`]. Three jobs: Repair (the machine's setup: `doze dns-setup`,
//! `doze trust` in each stack that declares domains, the `PATH` link, stale routes), clearing what stacks left behind,
//! and `doze doctor` itself. Each step that needs root asks once, with the system's own prompt.

use std::path::{Path, PathBuf};

use super::cli::{Cli, Verb};
use super::leftovers::{self, Cleared};
use super::repair::{self, Fail, Hands, Outcome, Step};
use crate::view::StackView;

/// One thing a job will do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Planned {
    /// A doze command, as the person, in a folder (none: the doze home).
    Doze {
        what: String,
        dir: Option<PathBuf>,
        args: Vec<String>,
    },
    /// A script run as root, after the system's password prompt.
    Admin { what: String, script: String },
    /// A process stopped by its exact pid.
    Stop { what: String, pid: u32 },
    /// A folder deleted.
    Delete { what: String, dir: PathBuf },
}

impl Planned {
    /// What it is for, in a few words.
    pub fn what(&self) -> &str {
        match self {
            Planned::Doze { what, .. }
            | Planned::Admin { what, .. }
            | Planned::Stop { what, .. }
            | Planned::Delete { what, .. } => what,
        }
    }

    /// Whether it asks for the administrator's password: a script as root, or a doze command whose step is root's.
    pub fn asks(&self, cli: &Cli) -> bool {
        match self {
            Planned::Admin { script, .. } => !script.trim().is_empty(),
            Planned::Doze { args, .. } => cli.asks_root(args),
            Planned::Stop { .. } | Planned::Delete { .. } => false,
        }
    }

    /// The command as it will run, as a person would type it.
    pub fn command(&self, cli: &Cli) -> String {
        match self {
            Planned::Doze { args, .. } => cli.line(args),
            Planned::Admin { script, .. } => script.clone(),
            Planned::Stop { pid, .. } => {
                format!("kill -TERM {pid}   (then -KILL if it is still there after 5 s)")
            }
            Planned::Delete { dir, .. } => {
                format!("rm -r {}", super::quote(&dir.to_string_lossy()))
            }
        }
    }
}

/// A privileged action the app runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    /// The machine's setup: what each failing part's fix does in a terminal.
    Repair(Vec<Step>),
    /// What stacks left behind: one leftover by its id, or `all` but the data.
    Clear(String),
    /// `doze doctor`, its report as it prints it, in a stack's folder (doctor reads the names setting from one).
    Doctor(Option<PathBuf>),
}

impl Job {
    /// The sheet's title.
    pub fn title(&self) -> String {
        match self {
            Job::Repair(_) => "Set up this machine".into(),
            Job::Clear(id) if id == "all" => "Clean up what stacks left behind".into(),
            Job::Clear(_) => "Clear a leftover".into(),
            Job::Doctor(_) => "Run doze doctor".into(),
        }
    }

    /// What a job with nothing to do says.
    pub fn nothing(&self) -> &'static str {
        match self {
            Job::Clear(_) => "nothing is left behind now",
            Job::Repair(_) | Job::Doctor(_) => "nothing to do: every part is in place",
        }
    }

    /// What it will do, step by step, doing nothing, and what it saw (the routes and leftovers it names): Run acts
    /// on those alone, never on something found later and not shown. `home` is the doze home, `admin` whether the
    /// app can ask for the password (leftovers needing it are refused without).
    pub fn plan(
        &self,
        cli: &Cli,
        home: &Path,
        stacks: &[StackView],
        admin: bool,
    ) -> (Vec<Planned>, Seen) {
        let mut seen = Seen::default();
        let planned = match self {
            Job::Repair(steps) => steps
                .iter()
                .map(|s| {
                    if s.prune {
                        let m = leftovers::gather(home, &[], true);
                        seen.routes.clone_from(&m.routes);
                        Planned::Admin {
                            what: s.what.clone(),
                            script: leftovers::route_script(&m.routes),
                        }
                    } else if !s.script.is_empty() {
                        Planned::Admin {
                            what: s.what.clone(),
                            script: s.script.clone(),
                        }
                    } else {
                        Planned::Doze {
                            what: s.what.clone(),
                            dir: s.dir.clone(),
                            args: s.args.clone(),
                        }
                    }
                })
                .collect(),
            Job::Clear(id) => {
                let m = leftovers::gather(home, stacks, admin);
                seen.leftovers = leftovers::chosen(id, &m);
                leftovers::plan_clear(id, &m)
            }
            Job::Doctor(dir) => vec![Planned::Doze {
                what: "the machine's report".into(),
                dir: dir.clone(),
                args: cli.args(Verb::Doctor),
            }],
        };
        (planned, seen)
    }
}

/// What a plan named: Run acts on these alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    /// The stale routes a repair's prune step showed.
    pub routes: Vec<String>,
    /// The leftovers (by id) a clean-up showed.
    pub leftovers: Vec<String>,
}

/// How a job ended: what to say, whether it is bad news, and a repair's outcome (its words wait for doctor's next
/// look, [`Outcome::words`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    pub said: String,
    pub bad: bool,
    pub outcome: Option<Outcome>,
}

/// Runs a job through `hands`, acting only on what its plan showed (`seen`): `(home, stacks, admin)` as for the plan.
pub fn run(
    job: &Job,
    cli: &Cli,
    hands: &mut dyn Hands,
    (home, stacks, admin): (&Path, &[StackView], bool),
    seen: &Seen,
) -> Ended {
    match job {
        Job::Repair(steps) => {
            let mut prune = |h: &mut dyn Hands| prune_routes(home, &seen.routes, h);
            let out = repair::run(steps, hands, &mut prune);
            let (said, bad) = match &out {
                Outcome::Done => ("every step ran; doctor is looking again".into(), false),
                other => other.words(&[]),
            };
            Ended {
                said,
                bad,
                outcome: Some(out),
            }
        }
        Job::Clear(_) => {
            let m = leftovers::gather(home, stacks, admin);
            let Cleared { said, bad } = leftovers::clear_these(&seen.leftovers, &m, hands);
            Ended {
                said,
                bad,
                outcome: None,
            }
        }
        Job::Doctor(dir) => {
            let (said, bad) = match hands.doze(dir.as_deref(), &cli.args(Verb::Doctor)) {
                Ok(()) => ("doze doctor found everything in place".into(), false),
                Err(Fail::Cancelled) => ("doze doctor was cancelled".into(), true),
                Err(Fail::Failed(why)) => (format!("doze doctor: {why}"), true),
            };
            Ended {
                said,
                bad,
                outcome: None,
            }
        }
    }
}

/// Removes the stale routes the plan showed and that are stale still, behind one prompt, then forgets them (a
/// repair's step when no stack runs doze trust).
fn prune_routes(home: &Path, shown: &[String], hands: &mut dyn Hands) -> Result<(), Fail> {
    let m = leftovers::gather(home, &[], true);
    let routes: Vec<String> = m.routes.into_iter().filter(|r| shown.contains(r)).collect();
    let script = leftovers::route_script(&routes);
    if script.is_empty() {
        return Ok(());
    }
    hands.admin("remove stale routes", &script)?;
    leftovers::forget_routes(home, &routes).map_err(|e| Fail::Failed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{Job, Planned};
    use crate::settings::cli::Cli;
    use crate::settings::repair::Step;

    fn step(what: &str, args: &[&str], script: &str) -> Step {
        Step {
            what: what.into(),
            dir: None,
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            script: script.into(),
            prune: false,
        }
    }

    #[test]
    fn a_repair_plans_its_commands_and_says_which_ask() {
        let cli = Cli::go("/x/doze".into());
        let job = Job::Repair(vec![
            step("the .doze names", &["dns-setup"], ""),
            step("put doze on your PATH", &[], "ln -sf a b"),
        ]);
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (p, _) = job.plan(&cli, dir.path(), &[], true);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].command(&cli), "doze dns-setup");
        assert!(p[0].asks(&cli));
        assert_eq!(
            p[1],
            Planned::Admin {
                what: "put doze on your PATH".into(),
                script: "ln -sf a b".into()
            }
        );
        assert_eq!(job.title(), "Set up this machine");
        let (doctor, _) = Job::Doctor(None).plan(&cli, dir.path(), &[], true);
        assert_eq!(doctor[0].command(&cli), "doze doctor");
        assert!(!doctor[0].asks(&cli));
    }
}
