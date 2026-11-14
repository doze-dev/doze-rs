//! Repair: the Machine table's one button (Go `serve/repair.go`). What each failing part's fix does in a terminal,
//! done from the app. The names and the domains are the CLI's own commands (`doze dns-setup`, `doze trust` in each
//! stack that declares domains), run as the person with `DOZE_ADMIN_PROMPT=1`, so their privileged step asks with the
//! system's administrator prompt instead of sudo and their logic stays in one place. Putting doze on the `PATH` is the
//! app's own script, through the same prompt (`doze_os::privilege::admin`). Each part that needs root asks once.
//!
//! The steps run through a [`Hands`], so a test (or a fixture) runs nothing on the machine.

use std::path::PathBuf;

use super::cli::{self, Flavour, Verb};
use super::machine::{PATH_LINK, Part};
use crate::view::StackView;

/// One step of a repair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// What is said: "the .doze names".
    pub what: String,
    /// A doze command, run in `dir` (none: the app's own folder).
    pub dir: Option<PathBuf>,
    pub args: Vec<String>,
    /// Or a script run as root.
    pub script: String,
    /// Or the stale routes removed by the app itself.
    pub prune: bool,
}

/// Why something the hands did failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fail {
    /// The person closed the administrator prompt.
    Cancelled,
    /// It failed; the words say why.
    Failed(String),
}

/// What carries a repair or a clean-up out: the CLI, the administrator prompt, processes and files. The app's is the
/// real machine's; a test's records what it was asked.
pub trait Hands {
    /// Runs `doze <args>` in `dir` with the prompt in place of sudo.
    fn doze(&mut self, dir: Option<&std::path::Path>, args: &[String]) -> Result<(), Fail>;
    /// Runs `script` as root after the system's prompt; `what` finishes "doze needs an administrator's password to …".
    fn admin(&mut self, what: &str, script: &str) -> Result<(), Fail>;
    /// Asks process `pid` to stop.
    fn terminate(&mut self, pid: u32) -> Result<(), Fail>;
    /// Stops process `pid` at once.
    fn kill(&mut self, pid: u32) -> Result<(), Fail>;
    /// Whether process `pid` is still there.
    fn alive(&mut self, pid: u32) -> bool;
    /// Waits a little (a tenth of a second) for a process to go.
    fn pause(&mut self);
    /// Deletes a folder and everything in it.
    fn remove_dir(&mut self, dir: &std::path::Path) -> Result<(), Fail>;
}

/// What a repair came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing was missing.
    Nothing,
    /// Every step ran.
    Done,
    /// The prompt was closed; the steps before it stand.
    Cancelled,
    /// A step failed.
    Failed { what: String, why: String },
}

impl Outcome {
    /// What the app says, given the parts as doctor sees them afterwards; and whether it is bad news.
    pub fn words(&self, after: &[Part]) -> (String, bool) {
        match self {
            Outcome::Nothing => ("nothing to repair: this machine is set up".into(), false),
            Outcome::Cancelled => ("repair cancelled; nothing more was changed".into(), false),
            Outcome::Failed { what, why } => (format!("setting up {what} failed: {why}"), true),
            Outcome::Done => {
                let still: Vec<&str> = after
                    .iter()
                    .filter(|p| !p.ok && !p.fix.is_empty())
                    .map(|p| p.name.as_str())
                    .collect();
                if still.is_empty() {
                    ("this machine is set up".into(), false)
                } else {
                    (
                        format!(
                            "repaired what could be; still missing: {}",
                            still.join(", ")
                        ),
                        true,
                    )
                }
            }
        }
    }
}

fn doze_step(what: String, dir: Option<PathBuf>, args: &[&str]) -> Step {
    Step {
        what,
        dir,
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        script: String::new(),
        prune: false,
    }
}

/// The steps that put the failing parts in place (Go `repairSteps`). `link` is the `doze` to put on the `PATH`, when
/// the app found one a terminal does not.
pub fn steps(stacks: &[StackView], parts: &[Part], link: Option<&std::path::Path>) -> Vec<Step> {
    let need = |s: &str| {
        parts
            .iter()
            .any(|p| p.section == s && !p.ok && !p.fix.is_empty())
    };
    let mut out = Vec::new();
    if need("names") {
        out.push(doze_step(
            "the .doze names".into(),
            None,
            cli::words(Flavour::default(), Verb::Names),
        ));
    }
    if need("domains") {
        let mut any = false;
        for st in stacks {
            let declares = st.instances.iter().any(|v| !v.domains.is_empty());
            if declares && !st.dir.is_empty() {
                any = true;
                let what = format!("{}'s domains", st.name);
                out.push(doze_step(
                    what,
                    Some(PathBuf::from(&st.dir)),
                    cli::words(Flavour::default(), Verb::Trust),
                ));
            }
        }
        // doze trust prunes stale routes in the same prompt; with no stack to run it in, the app prunes them itself.
        if !any
            && parts
                .iter()
                .any(|p| !p.ok && p.name.starts_with("stale route "))
        {
            out.push(Step {
                what: "remove stale routes".into(),
                dir: None,
                args: Vec::new(),
                script: String::new(),
                prune: true,
            });
        }
    }
    if let (true, Some(link)) = (need("app"), link) {
        out.push(Step {
            what: "put doze on your PATH".into(),
            dir: None,
            args: Vec::new(),
            script: format!(
                "mkdir -p /usr/local/bin\nln -sf {} {PATH_LINK}",
                super::quote(&link.to_string_lossy())
            ),
            prune: false,
        });
    }
    out
}

/// Runs the steps in order, stopping at the first that fails or is cancelled (Go `repair`). `prune` removes the stale
/// routes, for the step that asks it.
pub fn run(
    steps: &[Step],
    hands: &mut dyn Hands,
    prune: &mut dyn FnMut(&mut dyn Hands) -> Result<(), Fail>,
) -> Outcome {
    if steps.is_empty() {
        return Outcome::Nothing;
    }
    for st in steps {
        let done = if st.prune {
            prune(hands)
        } else if !st.script.is_empty() {
            hands.admin(&st.what, &st.script)
        } else {
            hands.doze(st.dir.as_deref(), &st.args)
        };
        match done {
            Ok(()) => {}
            Err(Fail::Cancelled) => return Outcome::Cancelled,
            Err(Fail::Failed(why)) => {
                return Outcome::Failed {
                    what: st.what.clone(),
                    why,
                };
            }
        }
    }
    Outcome::Done
}

/// What a doze command the app ran came to, from its output (Go `runStep`): cancelled when it says the prompt was;
/// else its last line says why.
pub fn doze_outcome(ok: bool, output: &str) -> Result<(), Fail> {
    if ok {
        return Ok(());
    }
    if output.contains(CANCELLED_TEXT) {
        return Err(Fail::Cancelled);
    }
    let last = output.trim().rsplit('\n').next().unwrap_or_default().trim();
    Err(Fail::Failed(if last.is_empty() {
        "it failed and said nothing".into()
    } else {
        last.to_owned()
    }))
}

/// What a cancelled prompt says, in the output of a doze command the app ran (Go `adminprompt.CancelledText`).
pub const CANCELLED_TEXT: &str = "the administrator prompt was cancelled";

/// The environment variable that has a doze command ask with the system's prompt (Go `adminprompt.Env`).
pub const PROMPT_ENV: &str = "DOZE_ADMIN_PROMPT";
