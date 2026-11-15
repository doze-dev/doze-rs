//! A stack action that did not work (Go `view.StackFailure`, `FailureOf`): what was tried, the lines it printed, and
//! what went wrong in one sentence. A mistake in doze.hcl becomes the file, the line and what to write; anything else,
//! its most telling line. It stays on the stack's page until dismissed or tried again.

use std::path::Path;
use std::time::SystemTime;

use crate::overview::{ConfigError, config_error, on_file};

/// How many printed lines are kept.
pub const LINES_KEPT: usize = 40;

/// A stack action that did not work.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Failure {
    /// "Couldn't start shop".
    pub what: String,
    /// What it printed, the last [`LINES_KEPT`].
    pub lines: Vec<String>,
    /// What went wrong, in one sentence.
    pub problem: String,
    /// Set when it is a mistake in doze.hcl.
    pub config: Option<Box<ConfigError>>,
}

impl Failure {
    /// The line said and kept among the recent ones: "Couldn't start shop: doze.hcl line 3, Unsupported block type".
    pub fn said(&self) -> String {
        match &self.config {
            Some(c) if c.line > 0 => {
                format!("{}: doze.hcl line {}, {}", self.what, c.line, self.problem)
            }
            _ => format!("{}: {}", self.what, self.problem),
        }
    }
}

// fuzz:parser fuzz_envelope
/// Reads what a stack action in `dir` printed when it failed.
pub fn failure_of(what: &str, dir: &Path, out: &str) -> Failure {
    let out = out.trim();
    let mut lines: Vec<String> = out.split('\n').map(str::to_owned).collect();
    if lines.len() > LINES_KEPT {
        lines.drain(..lines.len() - LINES_KEPT);
    }
    let mut f = Failure {
        what: what.to_owned(),
        lines,
        ..Failure::default()
    };
    if on_file(out).is_some() || out.contains("config has a mistake") {
        let now = SystemTime::now();
        let mut c = config_error(&dir.join("doze.hcl"), out, None, now);
        c.text.clear();
        f.problem.clone_from(&c.summary);
        f.config = Some(Box::new(c));
        return f;
    }
    // The telling line: the first that says error, else the last with words.
    let telling = out.split('\n').map(str::trim).find(|l| {
        let low = l.to_ascii_lowercase();
        low.starts_with("error") || low.contains("failed")
    });
    f.problem = match telling {
        Some(l) => l
            .strip_prefix("Error:")
            .or_else(|| l.strip_prefix("error:"))
            .unwrap_or(l)
            .trim()
            .to_owned(),
        None => out
            .split('\n')
            .map(str::trim)
            .rfind(|l| !l.is_empty())
            .unwrap_or_default()
            .to_owned(),
    };
    f
}

#[cfg(test)]
mod tests {
    use super::failure_of;
    use std::path::Path;

    #[test]
    fn the_telling_line_or_the_last() {
        let f = failure_of(
            "Couldn't start shop",
            Path::new("/nowhere"),
            "starting db…\nError: port 5432 is taken\nbye",
        );
        assert_eq!(f.problem, "port 5432 is taken");
        assert_eq!(f.lines.len(), 3);
        assert_eq!(f.said(), "Couldn't start shop: port 5432 is taken");
        let g = failure_of(
            "Couldn't stop shop",
            Path::new("/nowhere"),
            "doze: this is a fixture; shop cannot really stop here\n",
        );
        assert_eq!(
            g.problem,
            "doze: this is a fixture; shop cannot really stop here"
        );
    }

    #[test]
    fn a_config_mistake_names_the_line() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let f = failure_of(
            "Couldn't start shop",
            dir.path(),
            "Error: Unsupported block type\n\n  on doze.hcl line 3:\n   3: servce \"x\" {\n\nBlocks of type \"servce\" are not expected here.",
        );
        let c = f.config.as_ref().map(|c| (c.line, c.summary.clone()));
        assert_eq!(c, Some((3, "Unsupported block type".into())));
        assert_eq!(
            f.said(),
            "Couldn't start shop: doze.hcl line 3, Unsupported block type"
        );
    }

    #[test]
    fn only_the_last_forty_lines_are_kept() {
        let out: String = (0..50).map(|i| format!("line {i}\n")).collect();
        let f = failure_of("x", Path::new("/"), &out);
        assert_eq!(f.lines.len(), 40);
        assert_eq!(f.lines.first().map(String::as_str), Some("line 10"));
    }
}
