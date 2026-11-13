//! What the expression languages parse into. Names and values are already resolved: no placeholder survives parsing.

use std::collections::BTreeMap;

use crate::value::Value;

/// One step of a document path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Elem {
    /// A map key (or a top-level attribute name).
    Key(String),
    /// A list index.
    Index(usize),
}

/// A document path: `a.b[2].c`. The first element is always a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path(pub Vec<Elem>);

impl Path {
    /// The top-level attribute the path starts in.
    #[must_use]
    pub fn root(&self) -> &str {
        match self.0.first() {
            Some(Elem::Key(k)) => k,
            _ => "",
        }
    }

    /// The path as AWS prints it in an overlap message: `[a, b[2]]`.
    #[must_use]
    pub fn shown(&self) -> String {
        let mut out = String::from("[");
        for (i, e) in self.0.iter().enumerate() {
            match e {
                Elem::Key(k) => {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(k);
                }
                Elem::Index(n) => out.push_str(&format!("[{n}]")),
            }
        }
        out.push(']');
        out
    }

    /// Whether one path is the other or lies inside it.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.0.iter().zip(&other.0).all(|(a, b)| a == b)
    }
}

/// A value an expression computes with.
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    /// An attribute of the item.
    Path(Path),
    /// A literal from `ExpressionAttributeValues`.
    Value(Value),
    /// `size(path)`.
    Size(Path),
    /// `if_not_exists(path, operand)` (update expressions).
    IfNotExists(Path, Box<Operand>),
    /// `list_append(a, b)` (update expressions).
    ListAppend(Box<Operand>, Box<Operand>),
}

/// A comparison operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    /// `=`
    Eq,
    /// `<>`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

/// A function that answers true or false.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Func {
    /// `attribute_exists(path)`
    AttributeExists,
    /// `attribute_not_exists(path)`
    AttributeNotExists,
    /// `attribute_type(path, :type)`
    AttributeType,
    /// `begins_with(path, :prefix)`
    BeginsWith,
    /// `contains(path, operand)`
    Contains,
}

impl Func {
    /// The name as written.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AttributeExists => "attribute_exists",
            Self::AttributeNotExists => "attribute_not_exists",
            Self::AttributeType => "attribute_type",
            Self::BeginsWith => "begins_with",
            Self::Contains => "contains",
        }
    }
}

/// A condition or filter expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Cond {
    /// `a < b`
    Cmp(Cmp, Operand, Operand),
    /// `a BETWEEN lo AND hi`
    Between(Operand, Operand, Operand),
    /// `a IN (x, y)`
    In(Operand, Vec<Operand>),
    /// A boolean function.
    Func(Func, Vec<Operand>),
    /// `a AND b`
    And(Box<Cond>, Box<Cond>),
    /// `a OR b`
    Or(Box<Cond>, Box<Cond>),
    /// `NOT a`
    Not(Box<Cond>),
}

/// The right side of a `SET`.
#[derive(Clone, Debug, PartialEq)]
pub enum SetValue {
    /// `path = operand`
    Operand(Operand),
    /// `path = a + b`
    Plus(Operand, Operand),
    /// `path = a - b`
    Minus(Operand, Operand),
}

/// An update expression: its four clauses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Update {
    /// `SET path = value, ...`
    pub set: Vec<(Path, SetValue)>,
    /// `REMOVE path, ...`
    pub remove: Vec<Path>,
    /// `ADD path value, ...`
    pub add: Vec<(Path, Value)>,
    /// `DELETE path value, ...`
    pub delete: Vec<(Path, Value)>,
}

/// The names and values a request supplied, which expressions refer to by `#name` and `:value`.
pub type NameMap = BTreeMap<String, String>;
/// Expression attribute values by `:name`.
pub type ValueMap = BTreeMap<String, Value>;
