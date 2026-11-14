//! The command source: which `doze` the app runs for the machine's setup and for doctor, and with which words. One
//! place, so that when the Rust CLI takes over from the Go one only this file changes.
//!
//! Today the daemon and the CLI are Go's (`doze dns-setup`, `doze trust`, `doze doctor`, each with `--print` for the
//! script it would run as root, and `DOZE_ADMIN_PROMPT=1` to ask with the system's prompt instead of sudo). The
//! program is found as a terminal would find it, unless [`CLI_ENV`] names one: a later Rust CLI, or a stand-in that
//! echoes for a test.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::repair::PROMPT_ENV;

/// Names the `doze` program the app runs, in place of the one on the `PATH`.
pub const CLI_ENV: &str = "DOZE_APP_CLI";

/// Whose command line it is: the words differ between CLIs, the actions do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Flavour {
    /// The Go `doze` (today's daemon and CLI).
    #[default]
    Go,
}

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

/// The words that do `verb` in a CLI of this flavour.
pub fn words(flavour: Flavour, verb: Verb) -> &'static [&'static str] {
    match (flavour, verb) {
        (Flavour::Go, Verb::Names) => &["dns-setup"],
        (Flavour::Go, Verb::Trust) => &["trust"],
        (Flavour::Go, Verb::Doctor) => &["doctor"],
    }
}

/// The `doze` the app runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub program: PathBuf,
    pub flavour: Flavour,
}

impl Cli {
    /// The CLI: [`CLI_ENV`] when it is set, else the first `doze` on `path`, then where installers put it.
    pub fn find(named: Option<&OsStr>, path: Option<&OsStr>, home: Option<&Path>) -> Option<Cli> {
        if let Some(p) = named.filter(|p| !p.is_empty()) {
            return Some(Cli::go(PathBuf::from(p)));
        }
        let mut dirs: Vec<PathBuf> = path
            .map(|p| std::env::split_paths(p).collect())
            .unwrap_or_default();
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
        if let Some(h) = home {
            dirs.push(h.join(".local/bin"));
            dirs.push(h.join("go/bin"));
        }
        dirs.into_iter()
            .map(|d| d.join("doze"))
            .find(|p| p.is_file())
            .map(Cli::go)
    }

    /// The CLI from this process's environment.
    pub fn from_env() -> Option<Cli> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Cli::find(
            std::env::var_os(CLI_ENV).as_deref(),
            std::env::var_os("PATH").as_deref(),
            home.as_deref(),
        )
    }

    /// Go's `doze` at `program`.
    pub fn go(program: PathBuf) -> Cli {
        Cli {
            program,
            flavour: Flavour::Go,
        }
    }

    /// The arguments that do `verb`.
    pub fn args(&self, verb: Verb) -> Vec<String> {
        words(self.flavour, verb)
            .iter()
            .map(|w| (*w).to_owned())
            .collect()
    }

    /// The arguments that print what `args` would run as root, running nothing; `None` when they print nothing of
    /// the kind (doctor needs no root).
    ///
    /// Only where printing changes nothing: Go's `doze trust --print` records the stack's domains and may create the
    /// authority before it prints, so it is not run before the person presses Run.
    pub fn print_args(&self, args: &[String]) -> Option<Vec<String>> {
        let first = args.first()?.as_str();
        let quiet = match self.flavour {
            Flavour::Go => first == "dns-setup",
        };
        quiet.then(|| {
            let mut a = args.to_vec();
            a.push("--print".into());
            a
        })
    }

    /// Whether the command's privileged step asks for the password (once, with the system's prompt).
    pub fn asks_root(&self, args: &[String]) -> bool {
        let first = args.first().map(String::as_str).unwrap_or_default();
        match self.flavour {
            Flavour::Go => matches!(first, "dns-setup" | "trust"),
        }
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
    fn the_go_cli_speaks_go_words() {
        let c = Cli::go("/x/doze".into());
        assert_eq!(c.args(Verb::Names), ["dns-setup"]);
        assert_eq!(c.args(Verb::Trust), ["trust"]);
        assert_eq!(c.args(Verb::Doctor), ["doctor"]);
        assert_eq!(
            c.print_args(&c.args(Verb::Names)).unwrap_or_default(),
            ["dns-setup", "--print"]
        );
        assert_eq!(
            c.print_args(&c.args(Verb::Trust)),
            None,
            "trust --print writes; never before Run"
        );
        assert!(c.asks_root(&c.args(Verb::Trust)));
        assert!(!c.asks_root(&c.args(Verb::Doctor)));
        assert_eq!(c.print_args(&c.args(Verb::Doctor)), None);
        assert_eq!(c.line(&c.args(Verb::Names)), "doze dns-setup");
        assert_eq!(c.line(&["a b".into()]), "doze 'a b'");
        assert!(c.env().contains(&("DOZE_ADMIN_PROMPT", "1")));
    }

    #[test]
    fn a_named_cli_wins_over_the_path() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let on_path = dir.path().join("doze");
        std::fs::write(&on_path, "").unwrap_or_else(|e| panic!("{e}"));
        let found = Cli::find(None, Some(dir.path().as_os_str()), None);
        assert_eq!(found.map(|c| c.program), Some(on_path));
        let named = Cli::find(
            Some("/fake/doze".as_ref()),
            Some(dir.path().as_os_str()),
            None,
        );
        assert_eq!(named.map(|c| c.program), Some("/fake/doze".into()));
    }
}
