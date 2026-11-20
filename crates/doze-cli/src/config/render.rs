//! A config that does not load, in the proposal's style (decision O6): the words are re-styled at print time, the
//! positions are Go's to the column. doze-config keeps Go's text word for word (its oracle compares it); this is the
//! only place it is re-worded:
//!
//! - one `error[DZ0301]` per diagnostic, its summary as the message;
//! - the location as `file:line:column` (relative to where the person is), the source line, a caret under the span;
//! - a trailing `Did you mean "x"?` becomes the caret's label, the rest of the detail a note under it.

use crate::diag::{Diagnostic, Span, ids};
use doze_hcl::diag::{Range, Severity};
use std::path::Path;

/// The diagnostics of a failed load, the first as the error and the rest after it.
pub(crate) fn diagnose(e: &doze_config::Error, cwd: &Path) -> Diagnostic {
    let mut drawn = e
        .diags
        .0
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| one(d, None, cwd));
    let Some(mut first) = drawn.next() else {
        let id = if e.is_invalid() {
            ids::CONFIG_INVALID
        } else {
            ids::CONFIG_UNREADABLE
        };
        return Diagnostic::new(id, e.text.clone())
            .fix("check the config's files can be read, then: doze lint");
    };
    let more: Vec<Diagnostic> = drawn.collect();
    let file = e
        .diags
        .0
        .iter()
        .find(|d| d.severity == Severity::Error)
        .and_then(|d| d.file.as_deref())
        .map_or_else(|| "doze.hcl".to_owned(), |f| shown(f, cwd));
    let fix = match more.len() {
        0 => format!("correct it in {file}, then: doze lint"),
        n => format!("correct these {} in {file}, then: doze lint", n + 1),
    };
    first.more = (!more.is_empty()).then(|| more.into_boxed_slice());
    first.fix(fix)
}

/// A file that does not parse (`fmt` leaves it as it is): the refusal, then each syntax error in it.
pub(crate) fn unparsable(
    file: &Path,
    diags: &doze_hcl::diag::Diagnostics,
    cwd: &Path,
) -> Diagnostic {
    let name = file.display().to_string();
    let mut out = Diagnostic::new(
        ids::CONFIG_INVALID,
        format!(
            "{} does not parse, so it was not formatted",
            shown(&name, cwd)
        ),
    );
    let more: Vec<Diagnostic> = diags
        .0
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| one(d, Some(&name), cwd))
        .collect();
    out.more = (!more.is_empty()).then(|| more.into_boxed_slice());
    out.fix(format!(
        "correct the syntax in {}, then: doze fmt",
        shown(&name, cwd)
    ))
}

fn one(d: &doze_hcl::diag::Diagnostic, file: Option<&str>, cwd: &Path) -> Diagnostic {
    let (label, note) = split_hint(&d.detail);
    let mut out = Diagnostic::new(ids::CONFIG_INVALID, d.summary.clone());
    if let Some(range) = d.subject {
        let file = file.or(d.file.as_deref()).unwrap_or("doze.hcl");
        let source = std::fs::read_to_string(cwd.join(file)).ok();
        out = out.span(span(&shown(file, cwd), source.as_deref(), range).labelled(label));
    } else if !label.is_empty() {
        out = out.note(label);
    }
    if !note.is_empty() {
        out = out.note(note);
    }
    out
}

/// A detail's closing `Did you mean "x"?`, lower-cased for the caret, and the rest of it.
fn split_hint(detail: &str) -> (String, String) {
    let detail = detail.trim();
    match detail.rfind("Did you mean ") {
        Some(at) if detail.ends_with('?') => {
            let hint = detail
                .get(at..)
                .unwrap_or_default()
                .replacen("Did", "did", 1);
            let rest = detail.get(..at).unwrap_or_default().trim().to_owned();
            (hint, rest)
        }
        _ => (String::new(), detail.to_owned()),
    }
}

/// The span at Go's line and column, with the source line drawn when the file can be read.
fn span(file: &str, source: Option<&str>, r: Range) -> Span {
    let (line, column) = (us(r.start.line), us(r.start.column));
    let (start, end) = (us(r.start.byte), us(r.end.byte));
    let text = source
        .and_then(|s| s.split('\n').nth(line.saturating_sub(1)))
        .unwrap_or_default()
        .trim_end_matches('\r');
    let line_start = source.map_or(0, |s| line_offset(s, line));
    let from = start.saturating_sub(line_start);
    let to = if r.end.line == r.start.line {
        end.saturating_sub(line_start)
    } else {
        text.len()
    };
    let covered = text
        .get(from.min(text.len())..to.clamp(from.min(text.len()), text.len()))
        .unwrap_or_default();
    Span {
        file: file.to_owned(),
        line: line.max(1),
        column: column.max(1),
        len: covered.chars().count().max(1),
        text: text.to_owned(),
        label: String::new(),
    }
}

/// A position from the parser as an index.
fn us(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// The byte where 1-based `line` starts.
fn line_offset(source: &str, line: usize) -> usize {
    source
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum()
}

/// A path as the person would type it: relative to where they are when it is under there.
fn shown(file: &str, cwd: &Path) -> String {
    // doze-config names a file relative to where doze runs, or absolutely: both print the same way.
    crate::project::shown(&cwd.join(file), cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use doze_hcl::diag::Pos;

    fn range(line: u32, col: u32, byte: usize, len: u32) -> Range {
        let byte = u32::try_from(byte).unwrap();
        Range {
            start: Pos {
                line,
                column: col,
                byte,
            },
            end: Pos {
                line,
                column: col + len,
                byte: byte + len,
            },
        }
    }

    #[test]
    fn a_did_you_mean_moves_under_the_caret() {
        let (label, rest) = split_hint(
            "An argument named \"vesion\" is not expected here. Did you mean \"version\"?",
        );
        assert_eq!(label, "did you mean \"version\"?");
        assert_eq!(rest, "An argument named \"vesion\" is not expected here.");
        assert_eq!(split_hint("plain"), (String::new(), "plain".into()));
    }

    #[test]
    fn the_span_is_at_gos_line_and_column() {
        let src = "stack = \"s\"\nmodule valkey \"c\" {\n  vesion = 8\n}\n";
        let at = src.find("vesion").unwrap();
        let s = span("doze.hcl", Some(src), range(3, 3, at, 6));
        assert_eq!((s.line, s.column, s.len), (3, 3, 6));
        assert_eq!(s.text, "  vesion = 8");
        // No source: the location alone.
        let s = span("doze.hcl", None, range(9, 4, 500, 3));
        assert_eq!((s.line, s.column, s.text.as_str()), (9, 4, ""));
    }
}
