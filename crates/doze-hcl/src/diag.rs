//! Diagnostics: what went wrong and where, with positions as Go's HCL gives them (1-based line, 1-based
//! column counted in grapheme clusters, 0-based byte offset).

use std::fmt;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos {
    pub line: u32,
    pub column: u32,
    pub byte: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Range {
    pub start: Pos,
    pub end: Pos,
}

impl Range {
    /// From the start of `a` to the end of `b` (Go's `hcl.RangeBetween`).
    pub fn between(a: Range, b: Range) -> Range {
        Range {
            start: a.start.min(b.start),
            end: a.end.max(b.end),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub summary: String,
    pub detail: String,
    /// The span the diagnostic is about.
    pub subject: Option<Range>,
    /// A wider span to show around it.
    pub context: Option<Range>,
    /// The file both ranges are in, when the reader of several files knows it.
    pub file: Option<std::sync::Arc<str>>,
}

impl Diagnostic {
    pub fn error(
        summary: impl Into<String>,
        detail: impl Into<String>,
        subject: Range,
    ) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            summary: summary.into(),
            detail: detail.into(),
            subject: Some(subject),
            context: None,
            file: None,
        }
    }

    pub fn with_context(mut self, context: Range) -> Diagnostic {
        self.context = Some(context);
        self
    }

    pub fn in_file(mut self, file: &str) -> Diagnostic {
        self.file = Some(std::sync::Arc::from(file));
        self
    }
}

/// Diagnostics in the order they were found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics(pub Vec<Diagnostic>);

impl Diagnostics {
    pub fn has_errors(&self) -> bool {
        self.0.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn push(&mut self, d: Diagnostic) {
        self.0.push(d);
    }

    pub fn extend(&mut self, other: Diagnostics) {
        self.0.extend(other.0);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.0.iter()
    }
}

impl From<Diagnostic> for Diagnostics {
    fn from(d: Diagnostic) -> Diagnostics {
        Diagnostics(vec![d])
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(r) = self.subject {
            write!(f, "{}:{}: ", r.start.line, r.start.column)?;
        }
        f.write_str(&self.summary)?;
        if !self.detail.is_empty() {
            write!(f, "; {}", self.detail)?;
        }
        Ok(())
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, d) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{d}")?;
        }
        Ok(())
    }
}

impl Diagnostics {
    /// The same mistake reported twice (same summary, detail and subject) once, first seen first:
    /// a value evaluated by more than one pass reports its error each time.
    pub fn dedupe(self) -> Diagnostics {
        let mut seen = std::collections::HashSet::new();
        Diagnostics(
            self.0
                .into_iter()
                .filter(|d| seen.insert((d.summary.clone(), d.detail.clone(), d.subject)))
                .collect(),
        )
    }
}

impl Diagnostic {
    /// The source line the subject starts on, with the subject's column span on it (1-based,
    /// end exclusive, clamped to the line), for a renderer to underline.
    pub fn snippet<'s>(&self, src: &'s str) -> Option<(u32, &'s str, std::ops::Range<u32>)> {
        let r = self.subject?;
        let line_no = r.start.line;
        let line = src
            .split('\n')
            .nth(usize::try_from(line_no.checked_sub(1)?).ok()?)?
            .trim_end_matches('\r');
        let last = u32::try_from(crate::funcs::graphemes::count_bytes(line.as_bytes()))
            .unwrap_or(u32::MAX)
            + 1;
        let end = if r.end.line == line_no {
            r.end.column.max(r.start.column + 1)
        } else {
            last
        };
        Some((
            line_no,
            line,
            r.start.column..end.min(last.max(r.start.column + 1)),
        ))
    }

    /// Plain text in the shape of hcl's diagnostic writer: the summary, where it is with the line
    /// quoted, then the detail. Wording is the caller's (decision O6); positions are Go's.
    pub fn render(&self, file: &str, src: &str) -> String {
        let sev = match self.severity {
            Severity::Error => "Error",
            Severity::Warning => "Warning",
        };
        let mut out = format!("{sev}: {}\n", self.summary);
        if let Some((n, line, cols)) = self.snippet(src) {
            let width = n.to_string().len();
            out.push_str(&format!("\n  on {file} line {n}:\n  {n}: {line}\n"));
            let pad = " ".repeat(width + 1 + usize::try_from(cols.start).unwrap_or(1));
            let marks = "^".repeat(usize::try_from(cols.end - cols.start).unwrap_or(1).max(1));
            out.push_str(&format!("  {pad}{marks}\n"));
        }
        if !self.detail.is_empty() {
            out.push_str(&format!("\n{}\n", self.detail));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_line_and_underlines_the_subject() {
        let src = "a = 1\nb = [1, 2\n";
        let ds = crate::check::check(src.as_bytes(), "doze.hcl");
        let d = ds
            .iter()
            .next()
            .map(|d| d.render("doze.hcl", src))
            .unwrap_or_default();
        assert_eq!(
            d,
            "Error: Unterminated tuple constructor expression\n\n  on doze.hcl line 2:\n  2: b = [1, 2\n         ^\n\nThere is no corresponding closing bracket before the end of the file. This may be caused by incorrect bracket nesting elsewhere in this file.\n"
        );
    }

    #[test]
    fn dedupe_keeps_the_first_of_each() {
        let r = Range::default();
        let d = Diagnostic::error("x", "y", r);
        let ds = Diagnostics(vec![d.clone(), Diagnostic::error("z", "y", r), d]).dedupe();
        assert_eq!(ds.0.len(), 2);
    }

    #[test]
    fn a_snippet_never_panics_on_odd_ranges() {
        let mut d = Diagnostic::error("x", "", Range::default());
        for (line, col) in [(0, 0), (1, 0), (1, 99), (9, 1), (2, 3)] {
            d.subject = Some(Range {
                start: Pos {
                    line,
                    column: col,
                    byte: 0,
                },
                end: Pos {
                    line,
                    column: col,
                    byte: 0,
                },
            });
            let _ = d.render("f", "ab\ncdé\n");
        }
    }
}
