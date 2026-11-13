//! The system's administrator prompt (Go `internal/adminprompt`): a script run as root after the person types an
//! administrator's password into the system's own dialog, never into doze. macOS asks through `osascript` (`do shell
//! script … with administrator privileges`); a Linux desktop through `pkexec` (polkit's dialog). Elsewhere, and on a
//! Linux machine with no display, there is no prompt: the terminal's sudo is the way.
//!
//! The script and the sentence go to `osascript` as arguments to its run handler, never spliced into AppleScript
//! source, so nothing in them needs AppleScript quoting.

use std::process::Command;

/// Why the prompt did not run the script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminError {
    /// The person closed the prompt.
    Cancelled,
    /// This system has no prompt doze can use.
    Unsupported,
    /// It ran and failed; the words are what the script or the prompt said.
    Failed(String),
}

impl std::fmt::Display for AdminError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdminError::Cancelled => f.write_str("the administrator prompt was cancelled"),
            AdminError::Unsupported => {
                f.write_str("no administrator prompt on this system: run the command in a terminal")
            }
            AdminError::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for AdminError {}

/// The sentence the prompt shows: "doze needs an administrator's password to …".
pub fn prompt(what: &str) -> String {
    let what = what.trim().trim_end_matches('.').trim();
    if what.is_empty() {
        "doze needs an administrator's password to set this machine up.".into()
    } else {
        format!("doze needs an administrator's password to {what}.")
    }
}

/// `s` as one single-quoted shell word.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Whether this system has a prompt doze can use.
pub fn admin_supported() -> bool {
    imp::supported()
}

/// Runs `script` as root after the system's prompt. `what` finishes "doze needs an administrator's password to …".
pub fn admin(what: &str, script: &str) -> Result<(), AdminError> {
    if !imp::supported() {
        return Err(AdminError::Unsupported);
    }
    let out = imp::command(what, script)
        .output()
        .map_err(|e| AdminError::Failed(e.to_string()))?;
    if out.status.success() {
        return Ok(());
    }
    Err(imp::classify(
        out.status.code(),
        &String::from_utf8_lossy(&out.stderr),
    ))
}

/// `osascript` asking for the password and running the script as root.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // WHY: the macOS prompt; its test runs everywhere
fn osascript(what: &str, script: &str) -> Command {
    let mut c = Command::new("/usr/bin/osascript");
    c.args([
        "-e",
        "on run argv",
        "-e",
        "do shell script (item 1 of argv) with prompt (item 2 of argv) with administrator privileges",
        "-e",
        "end run",
    ])
    .arg(format!("/bin/sh -c {}", quote(script)))
    .arg(prompt(what));
    c
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // WHY: the macOS prompt's words
/// osascript's failure as words: "<pos>: execution error: <message> (<code>)", where -128 is "User canceled."
fn classify_osascript(stderr: &str) -> AdminError {
    let msg = stderr.trim();
    if msg.contains("(-128)") {
        return AdminError::Cancelled;
    }
    let msg = msg.split_once("execution error: ").map_or(msg, |(_, m)| m);
    let msg = match msg.rfind(" (") {
        Some(i) if i > 0 && msg.ends_with(')') => msg.get(..i).unwrap_or(msg),
        _ => msg,
    };
    if msg.is_empty() {
        AdminError::Failed("the administrator prompt failed".into())
    } else {
        AdminError::Failed(msg.to_owned())
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))] // WHY: the Linux prompt's words
/// pkexec's failure: it exits 126 when the dialog was dismissed and 127 when authorisation failed.
fn classify_pkexec(code: Option<i32>, stderr: &str) -> AdminError {
    if matches!(code, Some(126 | 127)) && !stderr.contains("not found") {
        return AdminError::Cancelled;
    }
    let msg = stderr.trim();
    AdminError::Failed(if msg.is_empty() {
        "the administrator prompt failed".into()
    } else {
        msg.to_owned()
    })
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{AdminError, Command};

    pub(super) fn supported() -> bool {
        true
    }

    pub(super) fn command(what: &str, script: &str) -> Command {
        super::osascript(what, script)
    }

    pub(super) fn classify(_code: Option<i32>, stderr: &str) -> AdminError {
        super::classify_osascript(stderr)
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{AdminError, Command};

    pub(super) fn supported() -> bool {
        let display = ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .any(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty()));
        display && crate::which::which("pkexec").is_some()
    }

    pub(super) fn command(_what: &str, script: &str) -> Command {
        let mut c = Command::new("pkexec");
        c.args(["/bin/sh", "-c", script]);
        c
    }

    pub(super) fn classify(code: Option<i32>, stderr: &str) -> AdminError {
        super::classify_pkexec(code, stderr)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod imp {
    use super::{AdminError, Command};

    pub(super) fn supported() -> bool {
        false
    }

    pub(super) fn command(_what: &str, _script: &str) -> Command {
        Command::new("false")
    }

    pub(super) fn classify(_code: Option<i32>, _stderr: &str) -> AdminError {
        AdminError::Unsupported
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The script reaches the shell byte for byte, whatever quotes and heredocs it holds, and the sentence is its own
    /// argument (Go `TestTheScriptIsPassedAsAnArgumentAndQuotedOnce`). Only `/bin/sh -c printf` runs: no prompt.
    #[test]
    fn the_script_is_an_argument_quoted_once() {
        let script = "set -e\nprintf 'a\\nb'\ncat <<'PLIST'\n<x a=\"1\">it's</x>\nPLIST\necho \"$HOME\" `x` $(y)";
        let c = osascript("route .doze", script);
        let args: Vec<String> = c
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args.last().map(String::as_str),
            Some(prompt("route .doze").as_str())
        );
        assert!(args.iter().rev().skip(2).all(|a| !a.contains("PLIST")));
        let line = &args[args.len() - 2];
        let quoted = line.strip_prefix("/bin/sh -c ").unwrap();
        let out = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("printf %s {quoted}"))
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), script);
    }

    #[test]
    fn cancel_and_failure_are_told() {
        assert_eq!(
            classify_osascript("0:120: execution error: User canceled. (-128)\n"),
            AdminError::Cancelled
        );
        assert_eq!(
            classify_osascript("0:149: execution error: launchctl: no such file (5)\n"),
            AdminError::Failed("launchctl: no such file".into())
        );
        assert_eq!(classify_pkexec(Some(126), ""), AdminError::Cancelled);
        assert_eq!(
            classify_pkexec(Some(127), "pkexec: sh: not found"),
            AdminError::Failed("pkexec: sh: not found".into())
        );
        assert_eq!(
            prompt("route .doze to its resolver."),
            "doze needs an administrator's password to route .doze to its resolver."
        );
    }
}
