//! The doze this app is. Doze.app carries the whole CLI (owner decision, 2026-11-14), so `doze-app <command> …` is the
//! CLI, and the app runs itself for every doze command it needs (status, doctor, up, the machine's setup): a `doze`
//! on the `PATH` may be another, older one, whose words and answers are not this app's, and is never run.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The app's own first words; every other doze command is the CLI's.
const APP_WORDS: &[&str] = &["open", "shot", "bench", "fixture-serve"];

/// The CLI's flags for every command (doze-cli's `Global`) that the app's runners put before the command's word
/// (`doze --json -y up --last`). The app's own flags (`--show`, `--menu-bar`, `--fixture`, `--synth`) are none of them.
const CLI_SWITCHES: &[&str] = &["--json", "-q", "--quiet", "-y", "--yes", "--no-color"];
/// Those of them that take a value as the next argument.
const CLI_VALUED: &[&str] = &["-c", "--config", "--var", "--timeout"];

/// The name the bundle gives the CLI: a link to this program beside it (`xtask app-bundle`). Run under that name, the
/// program is the CLI and nothing else.
pub(crate) const CLI_NAME: &str = "doze";

/// Runs the command line as the CLI when it is the CLI's, and says its exit code.
pub(crate) fn as_cli() -> Option<u8> {
    let argv: Vec<OsString> = std::env::args_os().collect();
    is_cli(&argv).then(|| doze_cli::process::main(argv))
}

/// Whether `argv` (with the program first) is a CLI command line: the program is named `doze`, or the first word
/// after the CLI's flags is a doze command that is not one of the app's words (or is, after a CLI flag:
/// `doze-app --json open` is the CLI's `open`).
pub(crate) fn is_cli(argv: &[OsString]) -> bool {
    let named = argv
        .first()
        .map(Path::new)
        .and_then(Path::file_stem)
        .is_some_and(|s| s == CLI_NAME);
    named
        || first_word(argv.get(1..).unwrap_or_default()).is_some_and(|(word, flagged)| {
            doze_cli::is_command(word) && (flagged || !APP_WORDS.contains(&word))
        })
}

/// The first argument that is not one of the CLI's flags, and whether any came before it.
fn first_word(args: &[OsString]) -> Option<(&str, bool)> {
    let mut flagged = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let a = a.to_str()?;
        if CLI_SWITCHES.contains(&a) || CLI_VALUED.iter().any(|v| a.starts_with(&format!("{v}="))) {
            flagged = true;
        } else if CLI_VALUED.contains(&a) {
            flagged = true;
            it.next();
        } else {
            return Some((a, flagged));
        }
    }
    None
}

/// The program the app runs as its CLI: itself, under the bundle's `doze` link when there is one beside it (so a
/// command line it shows reads `doze …`, and the program is the CLI whatever the words). `None` under `cargo test`,
/// where this process is the test harness, not the app.
pub(crate) fn program() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    Some(linked(&exe).unwrap_or(exe))
}

/// The `doze` beside `exe` when it is `exe` itself (a link to it).
fn linked(exe: &Path) -> Option<PathBuf> {
    let link = exe.with_file_name(format!("{CLI_NAME}{}", std::env::consts::EXE_SUFFIX));
    let same = std::fs::canonicalize(&link).ok()? == std::fs::canonicalize(exe).ok()?;
    same.then_some(link)
}

#[cfg(test)]
mod tests {
    use super::{is_cli, linked};
    use std::ffi::OsString;

    fn line(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    /// The CLI's commands and aliases go to the CLI; the app's own words, folders and links stay the app's.
    #[test]
    fn doze_commands_run_the_cli() {
        for w in [
            "status",
            "ps",
            "logs",
            "wake",
            "boot",
            "why",
            "help",
            "version",
            "__refresh",
            "dns-setup",
        ] {
            assert!(is_cli(&line(&["doze-app", w])), "{w}");
        }
        for w in [
            "open",
            "shot",
            "bench",
            "fixture-serve",
            ".",
            "/Users/me/shop",
            "doze://shop",
            "--menu-bar",
            "nosuch",
        ] {
            assert!(!is_cli(&line(&["doze-app", w])), "{w}");
        }
        assert!(!is_cli(&line(&["doze-app"])));
    }

    /// The app's runners put the CLI's flags first (`--json -y up --last`, `--json status`): still the CLI's.
    #[test]
    fn the_cli_flags_before_the_word_are_the_cli() {
        for l in [
            &["doze-app", "--json", "status"][..],
            &["doze-app", "--json", "-y", "up", "--last"],
            &["doze-app", "-c", "x/doze.hcl", "up"],
            &["doze-app", "--var=a=b", "--timeout", "5s", "down"],
            &["doze-app", "--json", "open"],
        ] {
            assert!(is_cli(&line(l)), "{l:?}");
        }
        for l in [
            &["doze-app", "--json"][..],
            &["doze-app", "--show", "status"],
            &["doze-app", "--fixture", "shop"],
            &["doze-app", "--json", "nosuch"],
        ] {
            assert!(!is_cli(&line(l)), "{l:?}");
        }
    }

    /// Under the bundle's `doze` link the program is the CLI, whatever the words (`doze open`, `doze` alone).
    #[test]
    fn named_doze_is_the_cli() {
        assert!(is_cli(&line(&[
            "/Applications/Doze.app/Contents/MacOS/doze"
        ])));
        assert!(is_cli(&line(&["doze", "open"])));
        assert!(!is_cli(&line(&["/x/doze-app", "open"])));
    }

    #[cfg(unix)]
    #[test]
    fn a_doze_link_beside_the_app_is_its_cli() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let exe = dir.path().join("doze-app");
        std::fs::write(&exe, "").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(linked(&exe), None, "no link");
        std::fs::write(dir.path().join("doze"), "").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            linked(&exe),
            None,
            "another program named doze is not this one"
        );
        std::fs::remove_file(dir.path().join("doze")).unwrap_or_else(|e| panic!("{e}"));
        std::os::unix::fs::symlink("doze-app", dir.path().join("doze"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(linked(&exe), Some(dir.path().join("doze")));
    }
}
