//! The command source: which `doze` the app runs for the machine's setup and for doctor, and with which words.
//!
//! The app carries the CLI, so the program it runs is always itself ([`Cli::find`]'s `own`: the bundle's `doze` link
//! to the app). A `doze` on the `PATH` is never run in its place, nor one where installers put it: it may be another,
//! older one, whose words and answers are not the app's. When the app cannot name itself there is no CLI, and the
//! callers say so. [`CLI_ENV`] names a stand-in for a test or a check script only (a throwaway build of this same
//! doze, or a script that echoes), spoken to with the same words. The words are `doze dns-setup`, `doze trust` and
//! `doze doctor`, with `DOZE_ADMIN_PROMPT=1` to ask with the system's prompt instead of sudo; `--dry-run` prints the
//! root script and runs nothing.

use std::ffi::OsStr;
use std::path::PathBuf;

use super::repair::PROMPT_ENV;

/// Test-only: names a stand-in `doze` the app runs in place of itself (the check scripts' throwaway build, a unit
/// test's echoing script). Never set in a shipped app.
pub const CLI_ENV: &str = "DOZE_APP_CLI";

/// How a dry run (`doze dns-setup --dry-run`) marks the part it runs as the person, not under sudo: macOS's trust
/// settings, which macOS asks for in its own dialog. The run sheet says so beside the script.
pub const AS_YOU_MARK: &str = "as you, not under sudo";

/// What the app asks the CLI for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// The `.doze` names (resolver, network): `doze dns-setup`.
    Names,
    /// A stack's own domains (routes, the authority's trust): `doze trust`, in the stack's folder.
    Trust,
    /// The machine's report: `doze doctor`.
    Doctor,
}

/// The words that do `verb`.
pub fn words(verb: Verb) -> &'static [&'static str] {
    match verb {
        Verb::Names => &["dns-setup"],
        Verb::Trust => &["trust"],
        Verb::Doctor => &["doctor"],
    }
}

/// The `doze` the app runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub program: PathBuf,
}

impl Cli {
    /// The CLI: the test stand-in [`CLI_ENV`] names when it is set, else the app's `own` program; `None` when the app
    /// cannot name itself (never a `doze` looked for elsewhere).
    pub fn find(named: Option<&OsStr>, own: Option<PathBuf>) -> Option<Cli> {
        named
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or(own)
            .map(Cli::at)
    }

    /// The CLI from this process's environment, `own` being the program the app runs as its CLI (itself).
    pub fn from_env(own: Option<PathBuf>) -> Option<Cli> {
        Cli::find(std::env::var_os(CLI_ENV).as_deref(), own)
    }

    /// The CLI at `program`: the app itself, or a test's stand-in.
    pub fn at(program: PathBuf) -> Cli {
        Cli { program }
    }

    /// The arguments that do `verb`.
    pub fn args(&self, verb: Verb) -> Vec<String> {
        words(verb).iter().map(|w| (*w).to_owned()).collect()
    }

    /// The arguments that print what `args` would run as root, running nothing; `None` when they print nothing of
    /// the kind (doctor needs no root).
    ///
    /// Only where printing changes nothing: `doze trust` records the stack's domains and may create the authority
    /// before it prints, so it is not run before the person presses Run.
    pub fn print_args(&self, args: &[String]) -> Option<Vec<String>> {
        let first = args.first()?.as_str();
        (first == "dns-setup").then(|| {
            let mut a = args.to_vec();
            a.push("--dry-run".into());
            a
        })
    }

    /// Whether the command's privileged step asks for the password (once, with the system's prompt).
    pub fn asks_root(&self, args: &[String]) -> bool {
        let first = args.first().map(String::as_str).unwrap_or_default();
        matches!(first, "dns-setup" | "trust")
    }

    /// The environment every command the app runs gets: no colour, and the system's prompt in place of sudo.
    pub fn env(&self) -> [(&'static str, &'static str); 2] {
        [("NO_COLOR", "1"), (PROMPT_ENV, "1")]
    }

    /// The command as a person would type it: `doze dns-setup`.
    pub fn line(&self, args: &[String]) -> String {
        let mut out = String::from("doze");
        for a in args {
            out.push(' ');
            if a.is_empty() || a.contains(|c: char| c.is_whitespace() || "'\"$`\\".contains(c)) {
                out.push_str(&super::quote(a));
            } else {
                out.push_str(a);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{Cli, Verb};

    #[test]
    fn the_as_you_mark_is_the_one_a_dry_run_prints() {
        assert!(doze_names::setup::AS_YOU_NOTE.contains(super::AS_YOU_MARK));
        assert!(doze_names::setup::AS_YOU_FIRST_NOTE.contains(super::AS_YOU_MARK));
        assert_eq!(
            super::super::repair::MAC_DIALOG_CANCELLED_TEXT,
            doze_names::setup::MAC_DIALOG_CANCELLED
        );
        let out = format!(
            "doze needs sudo once\nerror[DZ0403]: {}\n",
            doze_names::setup::UserRefused::Cancelled(doze_names::setup::UserPart::Trust)
        );
        match super::super::repair::doze_outcome(false, &out) {
            Err(super::super::repair::Fail::Declined(what)) => {
                assert!(
                    what.starts_with("cancelled in macOS's dialog")
                        && what.contains("not trusted yet"),
                    "{what}"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_cli_speaks_its_words() {
        let c = Cli::at("/x/doze".into());
        assert_eq!(c.args(Verb::Names), ["dns-setup"]);
        assert_eq!(c.args(Verb::Trust), ["trust"]);
        assert_eq!(c.args(Verb::Doctor), ["doctor"]);
        assert_eq!(
            c.print_args(&c.args(Verb::Names)).unwrap_or_default(),
            ["dns-setup", "--dry-run"]
        );
        assert_eq!(
            c.print_args(&c.args(Verb::Trust)),
            None,
            "trust records before it prints; never before Run"
        );
        assert!(c.asks_root(&c.args(Verb::Names)));
        assert!(c.asks_root(&c.args(Verb::Trust)));
        assert!(!c.asks_root(&c.args(Verb::Doctor)));
        assert_eq!(c.print_args(&c.args(Verb::Doctor)), None);
        assert_eq!(c.line(&c.args(Verb::Names)), "doze dns-setup");
        assert_eq!(c.line(&["a b".into()]), "doze 'a b'");
        assert!(c.env().contains(&("DOZE_ADMIN_PROMPT", "1")));
    }

    #[test]
    fn the_app_runs_itself_and_nothing_found_elsewhere() {
        let own = Cli::find(
            None,
            Some("/Applications/Doze.app/Contents/MacOS/doze".into()),
        );
        assert_eq!(
            own,
            Some(Cli::at("/Applications/Doze.app/Contents/MacOS/doze".into()))
        );
        assert_eq!(
            Cli::find(None, None),
            None,
            "an app that cannot name itself has no CLI"
        );
        assert_eq!(Cli::find(Some("".as_ref()), None), None);
    }

    #[test]
    fn a_test_stand_in_replaces_the_app() {
        let named = Cli::find(Some("/fake/doze".as_ref()), Some("/app/doze".into()));
        assert_eq!(named.map(|c| c.program), Some("/fake/doze".into()));
    }
}
