//! JSONPath as Step Functions reads it (AWS uses Jayway's JsonPath). A *path* (`InputPath`, `OutputPath`, a `.$` field, `ItemsPath`)
//! may be the full language: wildcards, slices, unions, recursive descent and filters, and selects a list when it is not definite. A
//! *reference path* (`ResultPath`, and where a single node is required) names one node with field names and indices only.
//!
//! Paths start at `$` (the state's input), `$$` (the context object) or `$name` (a variable an earlier `Assign` wrote).

mod eval;
mod filter;
mod parse;

pub use eval::{Unreachable, set_at};
pub use filter::Filter;

/// Where a path starts.
#[derive(Clone, Debug, PartialEq)]
pub enum Root {
    /// `$`: the document the path is applied to.
    Input,
    /// `$$`: the context object.
    Context,
    /// `$name`: a variable.
    Var(String),
}

/// One step of a path.
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    /// `.a` or `['a']`.
    Field(String),
    /// `[2]` or `[-1]`.
    Index(i64),
    /// `.*` or `[*]`.
    Wild,
    /// `['a','b']`.
    Fields(Vec<String>),
    /// `[0,2]`.
    Indices(Vec<i64>),
    /// `[start:end:step]`.
    Slice(Option<i64>, Option<i64>, Option<i64>),
    /// `..` followed by a step: that step applied at every depth.
    Descend(Box<Seg>),
    /// `[?(...)]`.
    Filter(Box<Filter>),
    /// `.length()`: the size of an array, an object or a string.
    Length,
}

/// A parsed path.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub root: Root,
    pub segs: Vec<Seg>,
    /// As written, for messages that quote it.
    pub text: String,
}

/// What a path is read against.
#[derive(Clone, Copy, Debug)]
pub struct Scope<'a> {
    /// `$`.
    pub data: &'a serde_json::Value,
    /// `$$`.
    pub context: &'a serde_json::Value,
    /// `$name`: an object of the variables in scope.
    pub vars: &'a serde_json::Value,
}

impl Path {
    /// Reads a path.
    ///
    /// # Errors
    /// What is wrong with it.
    pub fn parse(text: &str) -> Result<Self, String> {
        parse::path(text)
    }

    /// Whether the path names at most one node (no wildcard, slice, union, descent or filter). A definite path selects a value; an
    /// indefinite one selects a list.
    #[must_use]
    pub fn definite(&self) -> bool {
        self.segs
            .iter()
            .all(|s| matches!(s, Seg::Field(_) | Seg::Index(_) | Seg::Length))
    }

    /// Whether the path is a reference path: rooted at `$`, field names and indices that are not negative.
    #[must_use]
    pub fn is_reference(&self) -> bool {
        self.root == Root::Input
            && self
                .segs
                .iter()
                .all(|s| matches!(s, Seg::Field(_)) || matches!(s, Seg::Index(i) if *i >= 0))
    }

    /// The value the path selects: one node for a definite path (`None` when there is none), and a list for any other (empty when
    /// nothing matched).
    #[must_use]
    pub fn select(&self, scope: Scope<'_>) -> Option<serde_json::Value> {
        eval::select(self, scope)
    }
}

/// Whether `text` reads as a path (for the definition checks).
#[must_use]
pub fn is_path(text: &str) -> bool {
    Path::parse(text).is_ok()
}

/// Whether `text` is a reference path.
#[must_use]
pub fn is_reference_path(text: &str) -> bool {
    Path::parse(text).is_ok_and(|p| p.is_reference())
}
