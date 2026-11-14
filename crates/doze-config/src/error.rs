//! Why a doze.hcl does not load: Go's `diagError` and `posErr` (diagnostics, deduplicated, with
//! doze's fix hints) or a refusal without a position. The text is Go's, word for word; the CLI
//! restyles it when it renders (plan item O6).

use doze_hcl::diag::{Diagnostic, Diagnostics, Range};
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

/// A range in a named file (Go's `hcl.Range`, which carries its file name).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct At {
    pub file: Arc<str>,
    pub range: Range,
}

impl At {
    pub fn new(file: &str, range: Range) -> At {
        At {
            file: Arc::from(file),
            range,
        }
    }
}

/// Go's `hcl.Range.String()`: `file:line,col-col`, or `file:line,col-line,col` across lines.
impl fmt::Display for At {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (s, e) = (self.range.start, self.range.end);
        if s.line == e.line {
            write!(f, "{}:{},{}-{}", self.file, s.line, s.column, e.column)
        } else {
            write!(
                f,
                "{}:{},{}-{},{}",
                self.file, s.line, s.column, e.line, e.column
            )
        }
    }
}

/// What kind of failure a load is (Go's `fault` kinds, as far as a load reaches).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The config as written does not load (`config_invalid`).
    Invalid,
    /// The files could not be found or read (Go returns that error as it is: `failed`).
    Failed,
}

/// A load that failed.
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub fault: Fault,
    /// The diagnostics Go would render, in its order (empty for a refusal without a position).
    pub diags: Diagnostics,
    /// The message, for a refusal without diagnostics; the rendered diagnostics otherwise.
    pub text: String,
}

impl Error {
    /// A refusal without a position (Go's `fmt.Errorf` errors, marked invalid).
    pub fn msg(text: impl Into<String>) -> Error {
        Error {
            fault: Fault::Invalid,
            diags: Diagnostics::default(),
            text: text.into(),
        }
    }

    pub fn failed(text: impl Into<String>) -> Error {
        Error {
            fault: Fault::Failed,
            ..Error::msg(text)
        }
    }

    pub fn is_invalid(&self) -> bool {
        self.fault == Fault::Invalid
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// The sources diagnostics are rendered against: file name to text.
pub trait Sources {
    fn source(&self, file: &str) -> Option<&str>;
}

/// Go's `posErr`: one error diagnostic at `at`.
pub fn pos_err(
    src: &dyn Sources,
    at: &At,
    summary: impl Into<String>,
    detail: impl Into<String>,
) -> Error {
    diag_error(
        src,
        Diagnostics::from(Diagnostic::error(summary, detail, at.range).in_file(&at.file)),
    )
}

/// Go's `diagError`: deduplicated, with the fix hints, rendered as hcl's text writer does.
pub fn diag_error(src: &dyn Sources, diags: Diagnostics) -> Error {
    let diags = add_fix_hints(dedupe(diags));
    let mut body = String::new();
    for d in diags.iter() {
        let file = d.file.as_deref().unwrap_or("");
        let text = src.source(file).unwrap_or("");
        body.push_str(&d.render(file, text));
        body.push('\n');
    }
    Error {
        fault: Fault::Invalid,
        text: format!(
            "the stack's config has a mistake —\n\n{}",
            body.trim_end_matches(['\n', ' ', '\t'])
        ),
        diags,
    }
}

/// Go's `dedupeDiags`: the same summary, detail and subject (file included) once.
fn dedupe(diags: Diagnostics) -> Diagnostics {
    let mut seen = HashSet::new();
    Diagnostics(
        diags
            .0
            .into_iter()
            .filter(|d| {
                seen.insert((
                    d.summary.clone(),
                    d.detail.clone(),
                    d.file.clone(),
                    d.subject,
                ))
            })
            .collect(),
    )
}

const SINGLE_LINE_HINT: &str = "\n\nHCL single-line blocks take exactly one argument; put each on its own line instead:\n    restart {\n      policy      = \"on_failure\"\n      max_retries = 5\n    }";

/// Go's `addFixHints` for the single-line block mistake. (The "Unsupported attribute" hint needs the
/// expression and its context, and is added where the expression is evaluated: `crate::eval`.)
fn add_fix_hints(mut diags: Diagnostics) -> Diagnostics {
    for d in &mut diags.0 {
        if d.summary.contains("single-argument block") && !d.detail.contains("own line") {
            d.detail.push_str(SINGLE_LINE_HINT);
        }
    }
    diags
}

/// Go's `nearest`: the closest candidate within an edit distance of 3, the first of equals.
pub fn nearest(s: &str, candidates: &[String]) -> Option<String> {
    let mut best: Option<(&String, usize)> = None;
    for c in candidates {
        let d = levenshtein(s, c);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((c, d));
        }
    }
    best.filter(|(_, d)| *d <= 3).map(|(c, _)| c.clone())
}

/// The edit distance between two strings, by runes (agext/levenshtein with default costs).
pub fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = Vec::with_capacity(b.len() + 1);
        cur.push(i + 1);
        for (j, cb) in b.iter().enumerate() {
            let sub = prev.get(j).copied().unwrap_or(0) + usize::from(ca != *cb);
            let del = prev.get(j + 1).copied().unwrap_or(0) + 1;
            let ins = cur.last().copied().unwrap_or(0) + 1;
            cur.push(sub.min(del).min(ins));
        }
        prev = cur;
    }
    prev.last().copied().unwrap_or(0)
}
