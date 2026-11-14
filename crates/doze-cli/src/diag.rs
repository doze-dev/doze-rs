//! What the CLI says when something stops it (rule 1: every message knows whose problem it is). A diagnostic has a
//! stable code, one sentence, the span when the problem is in a file, notes, and the fix: the command or change that
//! solves it. Never a library's error on its own.

pub mod ids;
pub mod snippet;

use crate::exit::Exit;
use crate::paint::Paint;
pub use ids::Id;
use serde::Serialize;
pub use snippet::Span;

/// One problem, in the reader's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub id: Id,
    pub message: String,
    pub span: Option<Box<Span>>,
    pub notes: Vec<String>,
    pub fix: Option<String>,
}

impl Diagnostic {
    pub fn new(id: Id, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            id,
            message: message.into(),
            span: None,
            notes: Vec::new(),
            fix: None,
        }
    }

    pub fn fix(mut self, fix: impl Into<String>) -> Diagnostic {
        self.fix = Some(fix.into());
        self
    }

    pub fn note(mut self, note: impl Into<String>) -> Diagnostic {
        self.notes.push(note.into());
        self
    }

    pub fn span(mut self, span: Span) -> Diagnostic {
        self.span = Some(Box::new(span));
        self
    }

    pub fn exit(&self) -> Exit {
        self.id.exit
    }

    /// The lines a person reads, on stderr.
    pub(crate) fn render(&self, p: Paint) -> String {
        let head = if self.id.exit == Exit::Partial {
            p.warn("!").into_owned()
        } else {
            format!("{}", p.fail(&format!("error[{}]:", self.id.code)))
        };
        let mut out = format!("{head} {}\n", p.bold(&self.message));
        match &self.span {
            Some(span) => {
                for line in span.draw(&self.notes) {
                    out.push_str(&p.dim(&line));
                    out.push('\n');
                }
            }
            None => {
                for n in &self.notes {
                    out.push_str(&format!("  {n}\n"));
                }
            }
        }
        if let Some(fix) = &self.fix {
            out.push_str(&format!("  {} {}\n", p.dim("fix:"), p.cmd(fix)));
        }
        out
    }

    /// The envelope's `error`: the published fields (`code`, `message`, `exit`) and the added ones.
    pub(crate) fn json(&self) -> ErrorBody<'_> {
        ErrorBody {
            code: self.id.exit.kind(),
            message: &self.message,
            exit: self.id.exit.code(),
            id: self.id.code,
            span: self.span.as_deref(),
            notes: &self.notes,
            fix: self.fix.as_deref(),
        }
    }
}

/// `error` in the `doze/v1` envelope. The first three fields are Go's and keep their meaning.
#[derive(Debug, Serialize)]
pub(crate) struct ErrorBody<'a> {
    code: &'static str,
    message: &'a str,
    exit: u8,
    id: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    span: Option<&'a Span>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    notes: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<&'a str>,
}
