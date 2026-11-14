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
        }
    }

    pub fn with_context(mut self, context: Range) -> Diagnostic {
        self.context = Some(context);
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
