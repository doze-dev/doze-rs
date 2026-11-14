use std::fmt;

/// Why a statement was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    /// Not PartiQL, or not PartiQL DynamoDB takes: a `ValidationException`.
    Syntax(String),
    /// PartiQL DynamoDB takes, that this build does not: refused by name.
    NotBuilt(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(m) => write!(f, "Statement wasn't well formed, can't be processed: {m}"),
            Self::NotBuilt(what) => write!(f, "PartiQL {what} is not built in doze-aws."),
        }
    }
}

pub(super) fn syntax<T>(m: impl Into<String>) -> Result<T, Error> {
    Err(Error::Syntax(m.into()))
}

/// One step of a path: an attribute (or map key) by name, or a list element by position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Name(String),
    Index(u32),
}

/// `a.b[2].c`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Path(pub(crate) Vec<Step>);

impl Path {
    /// The top-level attribute, when the path is nothing more.
    pub(crate) fn attribute(&self) -> Option<&str> {
        match self.0.as_slice() {
            [Step::Name(n)] => Some(n),
            _ => None,
        }
    }
}

/// A value written in the statement, or a `?` (the n-th parameter).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Lit {
    Param(usize),
    Str(String),
    Num(String),
    Bool(bool),
    Null,
    List(Vec<Lit>),
    Map(Vec<(String, Lit)>),
    /// `<<...>>`: a set.
    Bag(Vec<Lit>),
}

/// Something compared: an attribute, a value, or the size of an attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Operand {
    Path(Path),
    Value(Lit),
    Size(Path),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Cmp {
    pub(crate) const fn text(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Ne => "<>",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
        }
    }
}

/// The condition functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Func {
    BeginsWith,
    Contains,
    AttributeType,
}

/// A `WHERE` clause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Cmp(Operand, Cmp, Operand),
    Between(Operand, Operand, Operand),
    In(Operand, Vec<Operand>),
    Func(Func, Path, Operand),
    /// `IS MISSING` (`true`) or `IS NOT MISSING` (`false`).
    Missing(Path, bool),
    /// `IS NULL` (`true`) or `IS NOT NULL` (`false`).
    Null(Path, bool),
}

/// The right side of a `SET`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SetValue {
    Operand(Operand),
    Plus(Operand, Operand),
    Minus(Operand, Operand),
    ListAppend(Operand, Operand),
    IfNotExists(Path, Operand),
    /// `set_add(path, value)`, which must name the attribute it is assigned to.
    SetAdd(Path, Operand),
    SetDelete(Path, Operand),
}

/// `RETURNING ALL OLD *` and the rest: what an `UPDATE` or `DELETE` answers with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Returning {
    AllOld,
    AllNew,
    ModifiedOld,
    ModifiedNew,
}

impl Returning {
    pub(crate) const fn return_values(self) -> &'static str {
        match self {
            Self::AllOld => "ALL_OLD",
            Self::AllNew => "ALL_NEW",
            Self::ModifiedOld => "UPDATED_OLD",
            Self::ModifiedNew => "UPDATED_NEW",
        }
    }
}

/// One statement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Statement {
    Select {
        table: String,
        index: Option<String>,
        /// `None` for `*`.
        columns: Option<Vec<Path>>,
        filter: Option<Expr>,
    },
    Insert {
        table: String,
        item: Vec<(String, Lit)>,
    },
    Update {
        table: String,
        set: Vec<(Path, SetValue)>,
        remove: Vec<Path>,
        filter: Expr,
        returning: Option<Returning>,
    },
    Delete {
        table: String,
        filter: Expr,
        returning: Option<Returning>,
    },
}

impl Statement {
    pub(crate) fn table(&self) -> &str {
        match self {
            Self::Select { table, .. }
            | Self::Insert { table, .. }
            | Self::Update { table, .. }
            | Self::Delete { table, .. } => table,
        }
    }
}

/// A statement and how many `?` it has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Parsed {
    pub(crate) statement: Statement,
    pub(crate) params: usize,
}
