//! Version skew: a stack's daemon, or a test's stand-in CLI, that is older than the app. The app carries the CLI and
//! runs itself (doze-app `own`), but a daemon started before (by an older doze, the Go one included) keeps running its
//! own program; the app's one answer to it is "restart the stack", which stops it and brings the stack back up on the
//! app's own doze. Nothing is ever asked of an older doze beyond what it is. What a doze is
//! comes from `doze --json version` (`version`, the module `protocol`, and `commit` once the CLI says it); a doze that
//! cannot answer that (the Go doze: "unknown flag: --json") is older than any that can.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::settings::update::older;

/// What `doze --json version` said.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct DozeVersion {
    /// `v0.2.0`, or `dev` for a development build.
    pub version: String,
    /// The module protocol it speaks (0: it did not say).
    pub protocol: u16,
    /// The commit it was built from, when it says.
    pub commit: String,
}

/// Whose doze is older.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Whose {
    /// The program the stack's daemon runs.
    Daemon,
    /// The stand-in `doze` a test or check script named (`DOZE_APP_CLI`) in place of the app's own.
    Cli,
}

/// An older doze, as the stack page's banner says it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Skew {
    pub whose: Whose,
    /// The program.
    pub program: String,
    /// Its version as it said it, or `None` when it could not say.
    pub version: Option<String>,
    /// The banner's line.
    pub text: String,
    /// What it means, and the fix.
    pub detail: String,
}

/// Reads `doze --json version`'s envelope (`{"schema":"doze/v1","ok":true,"data":{…}}`), or `None` when it is not one.
pub fn read(stdout: &str) -> Option<DozeVersion> {
    #[derive(Deserialize)]
    struct Envelope {
        ok: bool,
        data: Option<DozeVersion>,
    }
    let e: Envelope = serde_json::from_str(stdout.trim()).ok()?;
    e.ok.then_some(e.data?).filter(|d| !d.version.is_empty())
}

/// What `program --json version` says, run with nothing of the app's environment that would change it.
pub fn ask(program: &Path) -> Option<DozeVersion> {
    let out = Command::new(program)
        .args(["--json", "version"])
        .env_remove("DOZE_APP_CHILD")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    read(&String::from_utf8_lossy(&out.stdout))
}

/// `other` is older than `own`: it could not say what it is, speaks an older module protocol, or is an older release.
/// Two development builds are not told apart (a dev build has no release number).
pub fn is_older(other: Option<&DozeVersion>, own: &DozeVersion) -> bool {
    match other {
        None => true,
        Some(o) => {
            // A doze that names no protocol predates the field.
            (own.protocol != 0 && o.protocol < own.protocol) || older(&o.version, &own.version)
        }
    }
}

/// The banner for `program`, whose doze said `other`, when it is older than the app's own doze.
pub fn skew(
    whose: Whose,
    program: &Path,
    other: Option<&DozeVersion>,
    own: &DozeVersion,
) -> Option<Skew> {
    if !is_older(other, own) {
        return None;
    }
    let shown = program.display().to_string();
    let mine = &own.version;
    let (text, detail) = match whose {
        Whose::Daemon => (
            match other {
                Some(o) => format!("This stack's daemon runs doze {}, older than the app's {mine}", o.version),
                None => format!("This stack's daemon runs an older doze than the app's {mine}"),
            },
            format!(
                "It was started by {shown}. Restart the stack to bring it back up on the app's own doze; data stays."
            ),
        ),
        Whose::Cli => (
            match other {
                Some(o) => format!("The app runs doze {} at {shown}, older than its own {mine}", o.version),
                None => format!("The app runs an older doze at {shown} than its own {mine}"),
            },
            "It runs in place of the doze the app carries (DOZE_APP_CLI names it). Unset DOZE_APP_CLI and open the app again."
                .to_owned(),
        ),
    };
    Some(Skew {
        whose,
        program: shown,
        version: other.map(|o| o.version.clone()),
        text,
        detail,
    })
}

/// The program process `pid` runs: `/proc/<pid>/exe` on Linux, `ps` elsewhere on Unix. `None` when it cannot be read.
pub fn program_of(pid: u32) -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        return std::fs::read_link(format!("/proc/{pid}/exe")).ok();
    }
    if !cfg!(unix) {
        return None;
    }
    // macOS's `comm` is the executable's full path.
    let out = Command::new("ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let path = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    path.is_absolute().then_some(path)
}

/// `a` and `b` are the same program (one may be a link to the other).
pub fn same_program(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::{DozeVersion, Whose, is_older, read, skew};
    use std::path::Path;

    fn v(version: &str, protocol: u16) -> DozeVersion {
        DozeVersion {
            version: version.into(),
            protocol,
            commit: String::new(),
        }
    }

    #[test]
    fn the_envelope_is_read_and_anything_else_is_not() {
        let rust = r#"{"schema":"doze/v1","command":"doze version","ok":true,"data":{"arch":"arm64","os":"darwin","protocol":2,"version":"dev"}}"#;
        assert_eq!(read(rust), Some(v("dev", 2)));
        let with_commit =
            r#"{"ok":true,"data":{"version":"v0.3.0","protocol":2,"commit":"abc123"}}"#;
        assert_eq!(read(with_commit).map(|d| d.commit), Some("abc123".into()));
        assert_eq!(read(""), None, "the Go doze prints nothing on stdout");
        assert_eq!(read("doze 0.1.1 (darwin/arm64, go1.26.4)"), None);
        assert_eq!(read(r#"{"ok":false,"error":{"message":"x"}}"#), None);
    }

    #[test]
    fn older_is_no_answer_an_older_protocol_or_an_older_release() {
        let own = v("v0.3.0", 2);
        assert!(is_older(None, &own), "the Go doze cannot say");
        assert!(is_older(Some(&v("v0.3.0", 1)), &own));
        assert!(is_older(Some(&v("v0.2.9", 2)), &own));
        assert!(
            is_older(Some(&v("v0.3.0", 0)), &own),
            "a doze that names no protocol predates it"
        );
        assert!(!is_older(Some(&v("v0.3.0", 2)), &own));
        assert!(!is_older(Some(&v("v0.4.0", 2)), &own), "newer is not skew");
        assert!(
            !is_older(Some(&v("dev", 2)), &v("dev", 2)),
            "two dev builds"
        );
        assert!(
            !is_older(Some(&v("v0.1.0", 2)), &v("dev", 2)),
            "a dev app has no number to compare"
        );
    }

    #[test]
    fn the_banner_names_the_program_and_the_fix() {
        let own = v("v0.3.0", 2);
        let d = skew(
            Whose::Daemon,
            Path::new("/opt/homebrew/bin/doze"),
            None,
            &own,
        );
        let d = d.unwrap_or_else(|| panic!("no skew"));
        assert_eq!(
            d.text,
            "This stack's daemon runs an older doze than the app's v0.3.0"
        );
        assert!(
            d.detail.contains("/opt/homebrew/bin/doze") && d.detail.contains("Restart the stack")
        );
        let c = skew(
            Whose::Cli,
            Path::new("/x/doze"),
            Some(&v("v0.2.0", 2)),
            &own,
        );
        let c = c.unwrap_or_else(|| panic!("no skew"));
        assert_eq!(
            c.text,
            "The app runs doze v0.2.0 at /x/doze, older than its own v0.3.0"
        );
        assert!(c.detail.contains("DOZE_APP_CLI"));
        assert_eq!(
            skew(Whose::Cli, Path::new("/x/doze"), Some(&own), &own),
            None
        );
    }
}
