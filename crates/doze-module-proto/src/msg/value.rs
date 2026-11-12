//! Evaluated configuration (PLAN.md D1): the core evaluates the block and sends a typed tree in which every node
//! carries the source span it came from, so a module's semantic error can point at the user's file.

use minicbor::{Decode, Encode};
use std::collections::BTreeMap;

/// A byte range in a source file the core knows by number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct Span {
    #[n(0)]
    pub file: u16,
    #[n(1)]
    pub start: u32,
    #[n(2)]
    pub end: u32,
}

/// A value. Numbers travel as their exact decimal text (cty numbers are 512-bit; see the HCL spike).
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub enum Value {
    #[n(0)]
    Null,
    #[n(1)]
    Bool(#[n(0)] bool),
    #[n(2)]
    Number(#[n(0)] String),
    #[n(3)]
    String(#[n(0)] String),
    #[n(4)]
    List(#[n(0)] Vec<Node>),
    #[n(5)]
    Map(#[n(0)] BTreeMap<String, Node>),
}

/// A value and where it came from.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct Node {
    #[n(0)]
    pub span: Span,
    #[n(1)]
    pub value: Value,
}

/// One step of a path into a value tree, for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum PathSeg {
    #[n(0)]
    Key(#[n(0)] String),
    #[n(1)]
    Index(#[n(0)] u32),
}

impl Node {
    /// A node with no source position (values the core made up, tests).
    pub fn bare(value: Value) -> Node {
        Node {
            span: Span::default(),
            value,
        }
    }

    /// The node at `key` of a map, if this is a map that has it.
    pub fn get(&self, key: &str) -> Option<&Node> {
        match &self.value {
            Value::Map(m) => m.get(key),
            _ => None,
        }
    }

    /// The span of the node at `path`, for pointing a diagnostic at it.
    pub fn span_at(&self, path: &[PathSeg]) -> Option<Span> {
        let Some((first, rest)) = path.split_first() else {
            return Some(self.span);
        };
        let child = match (first, &self.value) {
            (PathSeg::Key(k), Value::Map(m)) => m.get(k),
            (PathSeg::Index(i), Value::List(l)) => l.get(*i as usize),
            _ => None,
        }?;
        child.span_at(rest)
    }
}
