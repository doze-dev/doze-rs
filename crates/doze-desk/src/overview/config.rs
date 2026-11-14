//! A doze.hcl that does not load, read out where it went wrong (Go `view/configerror.go`): the summary and the line
//! from the loader's words, the lines around it from the file, and what to write instead. Also a doze.hcl saved after
//! the stack read it.
//!
//! The loader's words arrive as text (`doze --json status`'s error message, or a stack action's output), so they are
//! read here by hand where Go used three regular expressions.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use doze_display as words;
use serde::Serialize;

/// A doze.hcl that does not load.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigError {
    /// The banner's line: `doze.hcl changed 20 s ago and does not load`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// The loader's words: `Unsuitable value type`, `a number is required`.
    pub summary: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// What to write instead, for the mistakes doze knows.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hint: String,
    /// The file the loader pointed at (a sibling `*.doze.hcl` is merged with doze.hcl).
    pub file: String,
    /// The line it pointed at, 0 when none.
    #[serde(skip_serializing_if = "is_zero")]
    pub line: usize,
    /// The lines around it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub excerpt: Vec<SrcLine>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // WHY: serde's skip_serializing_if passes a reference
fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// One line of the excerpt.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SrcLine {
    pub n: usize,
    pub text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bad: bool,
}

impl ConfigError {
    /// What an instance page says of it (Go `InstancePage.Broken`): `doze.hcl does not load (line 9)`.
    #[must_use]
    pub fn broken(&self) -> String {
        if self.line > 0 {
            format!("doze.hcl does not load (line {})", self.line)
        } else {
            "doze.hcl does not load".to_owned()
        }
    }

    /// The mistake in one line: where, and what (Go `ConfigProblem`).
    pub fn problem(&self) -> String {
        if self.line > 0 {
            let name = Path::new(&self.file)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "doze.hcl".into());
            format!("{name} line {}: {}", self.line, self.summary)
        } else {
            self.summary.clone()
        }
    }
}

// fuzz:parser fuzz_hcl_scan
/// The loader's error `msg` about the doze.hcl at `path`, laid out (Go `ConfigErrorOf`). `changed` is when the file
/// was saved, for the banner; `None` when that is not known. The excerpt is read from the file, if it is there.
pub fn config_error(
    path: &Path,
    msg: &str,
    changed: Option<SystemTime>,
    now: SystemTime,
) -> ConfigError {
    let mut ce = ConfigError {
        file: path.display().to_string(),
        text: banner_text(changed, now),
        summary: summary(msg),
        ..ConfigError::default()
    };
    ce.detail = detail(msg, &ce.summary);
    if let Some((file, line)) = on_file(msg) {
        ce.line = line;
        let f = PathBuf::from(file);
        ce.file = if f.is_absolute() {
            f
        } else {
            path.parent().unwrap_or(Path::new("")).join(f)
        }
        .display()
        .to_string();
    } else if let Some(line) = after_word(msg, "line ") {
        ce.line = line;
    }
    ce.hint = hint(msg).to_owned();
    let own = ["write: ", "did you mean", "delete it"];
    if own.iter().any(|p| ce.detail.starts_with(p)) {
        // doze's own fixes are the hint.
        let mut c = ce.detail.chars();
        ce.hint = c
            .next()
            .map(|f| f.to_uppercase().chain(c).collect())
            .unwrap_or_default();
        ce.detail.clear();
    }
    if ce.line > 0
        && let Ok(src) = std::fs::read_to_string(&ce.file)
    {
        ce.excerpt = excerpt(&src, ce.line);
    }
    ce
}

/// The two lines before the bad one (three, as Go counts), the bad one, and two after.
fn excerpt(src: &str, line: usize) -> Vec<SrcLine> {
    let lines: Vec<&str> = src.split('\n').collect();
    let first = line.saturating_sub(3).max(1);
    let last = (line + 2).min(lines.len());
    (first..=last)
        .filter_map(|n| {
            lines.get(n - 1).map(|t| SrcLine {
                n,
                text: t.replace('\t', "  "),
                bad: n == line,
            })
        })
        .collect()
}

/// The banner's line for a file saved at `changed`.
fn banner_text(changed: Option<SystemTime>, now: SystemTime) -> String {
    match changed {
        None => "doze.hcl does not load".to_owned(),
        Some(t) => format!(
            "doze.hcl changed {} ago and does not load",
            words::ago(now.duration_since(t).unwrap_or_default())
        ),
    }
}

/// `Error: <summary>` on a line of its own, else the first line of the humanised error.
fn summary(msg: &str) -> String {
    msg.lines()
        .find_map(|l| l.strip_prefix("Error: "))
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| words::first_line(words::humanise(msg)).to_owned())
}

/// The last line with words that is not the quoted source, the `on …` line or the summary itself.
fn detail(msg: &str, summary: &str) -> String {
    for l in msg.trim().lines().rev().map(str::trim) {
        if l.is_empty() || numbered(l) || l.starts_with("on ") {
            continue;
        }
        if l != format!("Error: {summary}") {
            return l.to_owned();
        }
        break;
    }
    String::new()
}

/// The loader's quoted source line: `5:   port = …`.
fn numbered(l: &str) -> bool {
    let digits = l.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && l.as_bytes().get(digits) == Some(&b':')
}

/// `on FILE line N`: the file and the line.
pub(crate) fn on_file(msg: &str) -> Option<(&str, usize)> {
    let mut rest = msg;
    while let Some(at) = rest.find("on ") {
        let after = rest.get(at + 3..)?;
        let file_end = after.find(char::is_whitespace)?;
        let (file, tail) = after.split_at(file_end);
        if !file.is_empty()
            && let Some(n) = tail.strip_prefix(" line ").and_then(leading_number)
        {
            return Some((file, n));
        }
        rest = after;
    }
    None
}

/// The number after the first `word` in `msg`.
fn after_word(msg: &str, word: &str) -> Option<usize> {
    let mut rest = msg;
    while let Some(at) = rest.find(word) {
        let tail = rest.get(at + word.len()..)?;
        if let Some(n) = leading_number(tail) {
            return Some(n);
        }
        rest = tail;
    }
    None
}

fn leading_number(s: &str) -> Option<usize> {
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// What to write instead, for the errors people meet most.
fn hint(msg: &str) -> &'static str {
    let has = |s: &str| msg.contains(s);
    if has("a number is required") {
        "Write the number without quotes: port = 8081."
    } else if has("a string is required") || has("string required") {
        "Put text in double quotes: run = \"pnpm dev\"."
    } else if has("a bool is required") {
        "Write true or false, without quotes."
    } else if has("Unsupported argument") {
        "That name is not one this block takes; check its spelling against the docs for the block."
    } else if has("Missing required argument") {
        "The block needs that argument; add it inside the block's braces."
    } else if has("Invalid single-argument block") {
        "A block on one line holds one argument; put each on its own line."
    } else if has("Unclosed configuration block") || has("Missing newline") {
        "A { is not closed: every block ends with } on a line of its own."
    } else {
        ""
    }
}

/// A running stack whose doze.hcl was saved after its daemon read it, and loads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ConfigChanged {
    /// `doze.hcl changed 2m ago, after the stack read it`.
    pub text: String,
}

/// Says so when the file saved at `mtime` is newer than `loaded`.
pub fn config_changed(
    mtime: SystemTime,
    loaded: Option<SystemTime>,
    now: SystemTime,
) -> Option<ConfigChanged> {
    let loaded = loaded?;
    (mtime > loaded).then(|| ConfigChanged {
        text: format!(
            "doze.hcl changed {} ago, after the stack read it",
            words::ago(now.duration_since(mtime).unwrap_or_default())
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::config_error;
    use std::path::Path;
    use std::time::UNIX_EPOCH;

    #[test]
    fn reads_the_loaders_words() {
        let msg = "the stack's config has a mistake —\n\nError: Unsuitable value type\n\n  on doze.hcl line 9, in app \"worker\":\n   9:   port  = \"eighty-eighty-one\"\n\nUnsuitable value: a number is required";
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("doze.hcl");
        let src: String = (1..=12).map(|n| format!("line {n}\n")).collect();
        std::fs::write(&file, src).unwrap();
        let ce = config_error(&file, msg, None, UNIX_EPOCH);
        assert_eq!(ce.summary, "Unsuitable value type");
        assert_eq!(ce.detail, "Unsuitable value: a number is required");
        assert_eq!(ce.line, 9);
        assert_eq!(ce.hint, "Write the number without quotes: port = 8081.");
        assert_eq!(ce.text, "doze.hcl does not load");
        assert_eq!(ce.problem(), "doze.hcl line 9: Unsuitable value type");
        assert_eq!(ce.broken(), "doze.hcl does not load (line 9)");
        let ns: Vec<usize> = ce.excerpt.iter().map(|l| l.n).collect();
        assert_eq!(ns, [6, 7, 8, 9, 10, 11]);
        assert!(ce.excerpt.iter().any(|l| l.bad && l.n == 9));
    }

    #[test]
    fn a_message_with_no_place() {
        let ce = config_error(Path::new("/x/doze.hcl"), "no such file", None, UNIX_EPOCH);
        assert_eq!(ce.line, 0);
        assert!(ce.excerpt.is_empty());
        assert_eq!(ce.problem(), ce.summary);
        assert_eq!(ce.broken(), "doze.hcl does not load");
    }
}
