//! The PartiQL that DynamoDB takes, read into a statement: `SELECT`, `INSERT`, `UPDATE` and `DELETE`, one at a time.
//!
//! Bounded: the model caps a statement at 8,192 characters, and nesting (parentheses, `NOT`, lists and maps) is cut off at
//! [`MAX_DEPTH`], so no input makes the parser recurse without end. What is recognised but not built (`ORDER BY`, `EXISTS`, functions
//! in a select list) is refused by name ([`Error::NotBuilt`]), never read as something else. Every message is `(memory)`.

use std::fmt;

/// How deep parentheses, `NOT`s, lists and maps may nest.
const MAX_DEPTH: usize = 32;
/// The model's limit on a statement's length.
const MAX_LEN: usize = 8192;

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

fn syntax<T>(m: impl Into<String>) -> Result<T, Error> {
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    /// A bare word: an attribute or a keyword.
    Word(String),
    /// `"..."`: always an attribute or table, never a keyword.
    Quoted(String),
    Str(String),
    Num(String),
    Param,
    Sym(&'static str),
}

/// The two-character symbols, tried before the one-character ones.
const SYMBOLS2: [&str; 6] = ["<<", ">>", "<=", ">=", "<>", "!="];
const SYMBOLS1: [&str; 17] = [
    ",", ".", "(", ")", "[", "]", "{", "}", ":", "*", "+", "-", ";", "=", "<", ">", "?",
];

/// The text as tokens.
struct Lexer {
    chars: Vec<char>,
    at: usize,
}

impl Lexer {
    fn at(&self, i: usize) -> Option<char> {
        self.chars.get(i).copied()
    }

    fn take_while(&mut self, keep: impl Fn(char) -> bool) -> String {
        let start = self.at;
        while self.at(self.at).is_some_and(&keep) {
            self.at += 1;
        }
        self.chars
            .get(start..self.at)
            .unwrap_or_default()
            .iter()
            .collect()
    }

    /// `12`, `1.5`, `.5`, `2e-3`. Whether it is a number DynamoDB takes is the item's check.
    fn number(&mut self) -> String {
        let mut n = self.take_while(|c| c.is_ascii_digit());
        if self.at(self.at) == Some('.') {
            self.at += 1;
            n.push('.');
            n.push_str(&self.take_while(|c| c.is_ascii_digit()));
        }
        if matches!(self.at(self.at), Some('e' | 'E')) {
            let sign = matches!(self.at(self.at + 1), Some('+' | '-'));
            let digit_at = self.at + 1 + usize::from(sign);
            if self.at(digit_at).is_some_and(|c| c.is_ascii_digit()) {
                n.extend(self.chars.get(self.at..digit_at).unwrap_or_default());
                self.at = digit_at;
                n.push_str(&self.take_while(|c| c.is_ascii_digit()));
            }
        }
        n
    }

    /// Text between `quote`s, a doubled quote standing for itself.
    fn quoted(&mut self, quote: char) -> Result<String, Error> {
        let mut s = String::new();
        self.at += 1;
        loop {
            match self.at(self.at) {
                None => return syntax("an unterminated quoted text"),
                Some(q) if q == quote && self.at(self.at + 1) == Some(quote) => {
                    s.push(quote);
                    self.at += 2;
                }
                Some(q) if q == quote => {
                    self.at += 1;
                    return Ok(s);
                }
                Some(other) => {
                    s.push(other);
                    self.at += 1;
                }
            }
        }
    }

    fn symbol(&mut self) -> Result<Tok, Error> {
        let two: String = self
            .chars
            .get(self.at..self.at + 2)
            .unwrap_or_default()
            .iter()
            .collect();
        if let Some(s) = SYMBOLS2.into_iter().find(|s| *s == two) {
            self.at += 2;
            return Ok(Tok::Sym(s));
        }
        let c = self.at(self.at).unwrap_or_default();
        let one = c.to_string();
        let s = SYMBOLS1
            .into_iter()
            .find(|s| *s == one)
            .map_or_else(|| syntax(format!("unexpected character '{c}'")), Ok)?;
        self.at += 1;
        Ok(if s == "?" { Tok::Param } else { Tok::Sym(s) })
    }

    fn next(&mut self) -> Result<Option<Tok>, Error> {
        loop {
            let Some(c) = self.at(self.at) else {
                return Ok(None);
            };
            if c.is_whitespace() {
                self.at += 1;
            } else if c == '-' && self.at(self.at + 1) == Some('-') {
                self.take_while(|c| c != '\n');
            } else {
                break;
            }
        }
        let c = self.at(self.at).unwrap_or_default();
        let tok = if c.is_ascii_alphabetic() || c == '_' {
            Tok::Word(self.take_while(|c| c.is_ascii_alphanumeric() || c == '_'))
        } else if c.is_ascii_digit()
            || (c == '.' && self.at(self.at + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            Tok::Num(self.number())
        } else if c == '"' {
            Tok::Quoted(self.quoted('"')?)
        } else if c == '\'' {
            Tok::Str(self.quoted('\'')?)
        } else {
            self.symbol()?
        };
        Ok(Some(tok))
    }
}

fn lex(text: &str) -> Result<Vec<Tok>, Error> {
    let mut lexer = Lexer {
        chars: text.chars().collect(),
        at: 0,
    };
    let mut out = Vec::new();
    while let Some(tok) = lexer.next()? {
        out.push(tok);
    }
    Ok(out)
}

// fuzz:parser fuzz_partiql
/// Reads one statement.
///
/// # Errors
/// [`Error::Syntax`] for what is not a statement DynamoDB takes, [`Error::NotBuilt`] for what this build does not do.
pub(crate) fn parse(text: &str) -> Result<Parsed, Error> {
    if text.chars().count() > MAX_LEN {
        return syntax(format!("a statement is at most {MAX_LEN} characters"));
    }
    let toks = lex(text)?;
    let mut p = Parser {
        toks,
        at: 0,
        params: 0,
        depth: 0,
    };
    let statement = p.statement()?;
    p.eat_sym(";");
    if let Some(t) = p.peek() {
        return syntax(format!("unexpected {} after the statement", describe(t)));
    }
    Ok(Parsed {
        statement,
        params: p.params,
    })
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Word(w) => format!("'{w}'"),
        Tok::Quoted(q) => format!("\"{q}\""),
        Tok::Str(s) => format!("'{s}'"),
        Tok::Num(n) => n.clone(),
        Tok::Param => "?".to_owned(),
        Tok::Sym(s) => format!("'{s}'"),
    }
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
    params: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn peek_at(&self, ahead: usize) -> Option<&Tok> {
        self.toks.get(self.at + ahead)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.at).cloned();
        if t.is_some() {
            self.at += 1;
        }
        t
    }

    fn is_word(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(w)) if w.eq_ignore_ascii_case(word))
    }

    fn eat_word(&mut self, word: &str) -> bool {
        let found = self.is_word(word);
        if found {
            self.at += 1;
        }
        found
    }

    fn word(&mut self, word: &str) -> Result<(), Error> {
        if self.eat_word(word) {
            return Ok(());
        }
        self.expected(word)
    }

    fn is_sym(&self, sym: &str) -> bool {
        matches!(self.peek(), Some(Tok::Sym(s)) if *s == sym)
    }

    fn eat_sym(&mut self, sym: &str) -> bool {
        let found = self.is_sym(sym);
        if found {
            self.at += 1;
        }
        found
    }

    fn sym(&mut self, sym: &str) -> Result<(), Error> {
        if self.eat_sym(sym) {
            return Ok(());
        }
        self.expected(&format!("'{sym}'"))
    }

    fn expected<T>(&self, what: &str) -> Result<T, Error> {
        match self.peek() {
            Some(t) => syntax(format!("expected {what}, found {}", describe(t))),
            None => syntax(format!("expected {what} at the end of the statement")),
        }
    }

    fn deeper(&mut self) -> Result<(), Error> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return syntax(format!("nesting deeper than {MAX_DEPTH}"));
        }
        Ok(())
    }

    fn statement(&mut self) -> Result<Statement, Error> {
        if self.eat_word("SELECT") {
            return self.select();
        }
        if self.eat_word("INSERT") {
            return self.insert();
        }
        if self.eat_word("UPDATE") {
            return self.update();
        }
        if self.eat_word("DELETE") {
            return self.delete();
        }
        if self.is_word("UPSERT") || self.is_word("REPLACE") {
            return Err(Error::NotBuilt("UPSERT and REPLACE"));
        }
        self.expected("SELECT, INSERT, UPDATE or DELETE")
    }

    /// A table or attribute name: a bare word, or a double-quoted one.
    fn name(&mut self) -> Result<String, Error> {
        match self.next() {
            Some(Tok::Word(w) | Tok::Quoted(w)) => Ok(w),
            Some(t) => syntax(format!("expected a name, found {}", describe(&t))),
            None => syntax("expected a name at the end of the statement"),
        }
    }

    fn select(&mut self) -> Result<Statement, Error> {
        let columns = if self.eat_sym("*") {
            None
        } else {
            let mut cols = vec![self.column()?];
            while self.eat_sym(",") {
                cols.push(self.column()?);
            }
            Some(cols)
        };
        self.word("FROM")?;
        let table = self.name()?;
        let index = if self.eat_sym(".") {
            Some(self.name()?)
        } else {
            None
        };
        let filter = if self.eat_word("WHERE") {
            Some(self.expr()?)
        } else {
            None
        };
        if self.is_word("ORDER") {
            return Err(Error::NotBuilt("ORDER BY"));
        }
        Ok(Statement::Select {
            table,
            index,
            columns,
            filter,
        })
    }

    fn column(&mut self) -> Result<Path, Error> {
        if matches!(self.peek(), Some(Tok::Word(_)))
            && matches!(self.peek_at(1), Some(Tok::Sym("(")))
        {
            return Err(Error::NotBuilt("functions in a SELECT list"));
        }
        self.path()
    }

    fn insert(&mut self) -> Result<Statement, Error> {
        self.word("INTO")?;
        let table = self.name()?;
        if self.is_sym(".") {
            return syntax("an item cannot be inserted into an index");
        }
        if self.is_word("VALUES") {
            return syntax("INSERT takes VALUE and one item, not VALUES");
        }
        self.word("VALUE")?;
        let Lit::Map(item) = self.literal()? else {
            return syntax("INSERT ... VALUE takes an item: {'attribute': value, ...}");
        };
        Ok(Statement::Insert { table, item })
    }

    fn update(&mut self) -> Result<Statement, Error> {
        let table = self.name()?;
        let (mut set, mut remove) = (Vec::new(), Vec::new());
        loop {
            if self.eat_word("SET") {
                loop {
                    let path = self.path()?;
                    self.sym("=")?;
                    let value = self.set_value(&path)?;
                    set.push((path, value));
                    if !self.eat_sym(",") {
                        break;
                    }
                }
            } else if self.eat_word("REMOVE") {
                loop {
                    remove.push(self.path()?);
                    if !self.eat_sym(",") {
                        break;
                    }
                }
            } else {
                break;
            }
        }
        if set.is_empty() && remove.is_empty() {
            return self.expected("SET or REMOVE");
        }
        self.word("WHERE")?;
        let filter = self.expr()?;
        let returning = self.returning()?;
        Ok(Statement::Update {
            table,
            set,
            remove,
            filter,
            returning,
        })
    }

    fn delete(&mut self) -> Result<Statement, Error> {
        self.word("FROM")?;
        let table = self.name()?;
        self.word("WHERE")?;
        let filter = self.expr()?;
        let returning = self.returning()?;
        if returning.is_some_and(|r| r != Returning::AllOld) {
            return syntax("a DELETE can return ALL OLD * only");
        }
        Ok(Statement::Delete {
            table,
            filter,
            returning,
        })
    }

    fn returning(&mut self) -> Result<Option<Returning>, Error> {
        if !self.eat_word("RETURNING") {
            return Ok(None);
        }
        let all = if self.eat_word("ALL") {
            true
        } else if self.eat_word("MODIFIED") {
            false
        } else {
            return self.expected("ALL or MODIFIED");
        };
        let new = if self.eat_word("NEW") {
            true
        } else if self.eat_word("OLD") {
            false
        } else {
            return self.expected("NEW or OLD");
        };
        self.sym("*")?;
        Ok(Some(match (all, new) {
            (true, false) => Returning::AllOld,
            (true, true) => Returning::AllNew,
            (false, false) => Returning::ModifiedOld,
            (false, true) => Returning::ModifiedNew,
        }))
    }

    fn set_value(&mut self, target: &Path) -> Result<SetValue, Error> {
        let func = match self.peek() {
            Some(Tok::Word(w)) if matches!(self.peek_at(1), Some(Tok::Sym("("))) => {
                Some(w.to_ascii_lowercase())
            }
            _ => None,
        };
        if let Some(f) = func {
            self.at += 2;
            let value = match f.as_str() {
                "list_append" => {
                    let a = self.operand()?;
                    self.sym(",")?;
                    SetValue::ListAppend(a, self.operand()?)
                }
                "if_not_exists" => {
                    let p = self.path()?;
                    self.sym(",")?;
                    SetValue::IfNotExists(p, self.operand()?)
                }
                "set_add" | "set_delete" => {
                    let p = self.path()?;
                    if &p != target {
                        return Err(Error::NotBuilt(
                            "set_add and set_delete on an attribute other than the one assigned",
                        ));
                    }
                    self.sym(",")?;
                    let v = self.operand()?;
                    if f == "set_add" {
                        SetValue::SetAdd(p, v)
                    } else {
                        SetValue::SetDelete(p, v)
                    }
                }
                _ => return syntax(format!("{f} is not a function a SET can use")),
            };
            self.sym(")")?;
            return Ok(value);
        }
        let first = self.operand()?;
        if self.eat_sym("+") {
            return Ok(SetValue::Plus(first, self.operand()?));
        }
        if self.eat_sym("-") {
            return Ok(SetValue::Minus(first, self.operand()?));
        }
        Ok(SetValue::Operand(first))
    }

    fn path(&mut self) -> Result<Path, Error> {
        let mut steps = vec![Step::Name(self.name()?)];
        loop {
            if self.is_sym(".") && matches!(self.peek_at(1), Some(Tok::Word(_) | Tok::Quoted(_))) {
                self.at += 1;
                steps.push(Step::Name(self.name()?));
            } else if self.eat_sym("[") {
                let Some(Tok::Num(n)) = self.next() else {
                    return syntax("a list position is a whole number");
                };
                let ix = n
                    .parse::<u32>()
                    .or_else(|_| syntax(format!("{n} is not a list position")))?;
                self.sym("]")?;
                steps.push(Step::Index(ix));
            } else {
                break;
            }
            if steps.len() > MAX_DEPTH {
                return syntax(format!("a path deeper than {MAX_DEPTH}"));
            }
        }
        Ok(Path(steps))
    }

    fn expr(&mut self) -> Result<Expr, Error> {
        self.deeper()?;
        let mut left = self.and()?;
        while self.eat_word("OR") {
            let right = self.and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        self.depth -= 1;
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr, Error> {
        let mut left = self.not()?;
        while self.eat_word("AND") {
            let right = self.not()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Expr, Error> {
        if self.eat_word("NOT") {
            self.deeper()?;
            let inner = self.not()?;
            self.depth -= 1;
            return Ok(Expr::Not(Box::new(inner)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, Error> {
        if self.eat_sym("(") {
            let inner = self.expr()?;
            self.sym(")")?;
            return Ok(inner);
        }
        if self.is_word("EXISTS") {
            return Err(Error::NotBuilt("EXISTS"));
        }
        if let Some(f) = self.condition_function()? {
            self.at += 2;
            let p = self.path()?;
            self.sym(",")?;
            let v = self.operand()?;
            self.sym(")")?;
            return Ok(Expr::Func(f, p, v));
        }
        let left = self.operand()?;
        if self.eat_word("BETWEEN") {
            let lo = self.operand()?;
            self.word("AND")?;
            let hi = self.operand()?;
            return Ok(Expr::Between(left, lo, hi));
        }
        if self.eat_word("IN") {
            return Ok(Expr::In(left, self.in_list()?));
        }
        if self.eat_word("IS") {
            return self.is(left);
        }
        let op = match self.next() {
            Some(Tok::Sym("=")) => Cmp::Eq,
            Some(Tok::Sym("<>" | "!=")) => Cmp::Ne,
            Some(Tok::Sym("<")) => Cmp::Lt,
            Some(Tok::Sym("<=")) => Cmp::Le,
            Some(Tok::Sym(">")) => Cmp::Gt,
            Some(Tok::Sym(">=")) => Cmp::Ge,
            Some(t) => return syntax(format!("expected a comparison, found {}", describe(&t))),
            None => return syntax("expected a comparison at the end of the statement"),
        };
        let right = self.operand()?;
        Ok(Expr::Cmp(left, op, right))
    }

    /// The condition function the next tokens call, if they call one (`size` is an operand, not a condition).
    fn condition_function(&self) -> Result<Option<Func>, Error> {
        let Some(Tok::Word(w)) = self.peek() else {
            return Ok(None);
        };
        if !matches!(self.peek_at(1), Some(Tok::Sym("("))) {
            return Ok(None);
        }
        match w.to_ascii_lowercase().as_str() {
            "begins_with" => Ok(Some(Func::BeginsWith)),
            "contains" => Ok(Some(Func::Contains)),
            "attribute_type" => Ok(Some(Func::AttributeType)),
            "size" => Ok(None),
            other => syntax(format!("{other} is not a condition function")),
        }
    }

    /// `[a, b]` or `(a, b)`, after `IN`.
    fn in_list(&mut self) -> Result<Vec<Operand>, Error> {
        let close = if self.eat_sym("[") {
            "]"
        } else if self.eat_sym("(") {
            ")"
        } else {
            return self.expected("'[' or '('");
        };
        let mut list = vec![self.operand()?];
        while self.eat_sym(",") {
            list.push(self.operand()?);
            if list.len() > 100 {
                return syntax("IN takes at most 100 values");
            }
        }
        self.sym(close)?;
        Ok(list)
    }

    /// `IS [NOT] MISSING` and `IS [NOT] NULL`, after `IS`.
    fn is(&mut self, left: Operand) -> Result<Expr, Error> {
        let negated = self.eat_word("NOT");
        let Operand::Path(p) = left else {
            return syntax("IS tests an attribute");
        };
        if self.eat_word("MISSING") {
            return Ok(Expr::Missing(p, !negated));
        }
        if self.eat_word("NULL") {
            return Ok(Expr::Null(p, !negated));
        }
        self.expected("MISSING or NULL")
    }

    fn operand(&mut self) -> Result<Operand, Error> {
        match self.peek() {
            Some(Tok::Word(w))
                if w.eq_ignore_ascii_case("size")
                    && matches!(self.peek_at(1), Some(Tok::Sym("("))) =>
            {
                self.at += 2;
                let p = self.path()?;
                self.sym(")")?;
                Ok(Operand::Size(p))
            }
            Some(Tok::Word(w))
                if ["true", "false", "null", "missing"]
                    .iter()
                    .any(|k| w.eq_ignore_ascii_case(k)) =>
            {
                Ok(Operand::Value(self.literal()?))
            }
            Some(Tok::Word(_) | Tok::Quoted(_)) => Ok(Operand::Path(self.path()?)),
            _ => Ok(Operand::Value(self.literal()?)),
        }
    }

    fn literal(&mut self) -> Result<Lit, Error> {
        let Some(t) = self.next() else {
            return syntax("expected a value at the end of the statement");
        };
        match t {
            Tok::Param => {
                self.params += 1;
                Ok(Lit::Param(self.params - 1))
            }
            Tok::Str(s) => Ok(Lit::Str(s)),
            Tok::Num(n) => Ok(Lit::Num(n)),
            Tok::Sym("-") => match self.next() {
                Some(Tok::Num(n)) => Ok(Lit::Num(format!("-{n}"))),
                _ => syntax("'-' must be followed by a number"),
            },
            Tok::Word(w) if w.eq_ignore_ascii_case("true") => Ok(Lit::Bool(true)),
            Tok::Word(w) if w.eq_ignore_ascii_case("false") => Ok(Lit::Bool(false)),
            Tok::Word(w) if w.eq_ignore_ascii_case("null") => Ok(Lit::Null),
            Tok::Word(w) if w.eq_ignore_ascii_case("missing") => {
                Err(Error::NotBuilt("MISSING as a value"))
            }
            Tok::Sym("[") => {
                self.deeper()?;
                let items = self.items("]")?;
                self.depth -= 1;
                Ok(Lit::List(items))
            }
            Tok::Sym("<<") => {
                self.deeper()?;
                let items = self.items(">>")?;
                self.depth -= 1;
                if items.is_empty() {
                    return syntax("a set cannot be empty");
                }
                Ok(Lit::Bag(items))
            }
            Tok::Sym("{") => {
                self.deeper()?;
                let mut fields = Vec::new();
                if !self.eat_sym("}") {
                    loop {
                        let key = match self.next() {
                            Some(Tok::Str(k)) => k,
                            _ => return syntax("a map key is a quoted text: {'key': value}"),
                        };
                        self.sym(":")?;
                        fields.push((key, self.literal()?));
                        if self.eat_sym("}") {
                            break;
                        }
                        self.sym(",")?;
                    }
                }
                self.depth -= 1;
                Ok(Lit::Map(fields))
            }
            other => syntax(format!("expected a value, found {}", describe(&other))),
        }
    }

    fn items(&mut self, close: &str) -> Result<Vec<Lit>, Error> {
        let mut out = Vec::new();
        if self.eat_sym(close) {
            return Ok(out);
        }
        loop {
            out.push(self.literal()?);
            if self.eat_sym(close) {
                return Ok(out);
            }
            self.sym(",")?;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

    use super::*;

    fn ok(text: &str) -> Statement {
        parse(text)
            .unwrap_or_else(|e| panic!("{text}: {e}"))
            .statement
    }

    fn name(n: &str) -> Path {
        Path(vec![Step::Name(n.to_owned())])
    }

    #[test]
    fn the_four_statements_read() {
        assert_eq!(
            ok(r#"SELECT * FROM "Music" WHERE Artist = ? AND "SongTitle" = 'Hi'"#),
            Statement::Select {
                table: "Music".into(),
                index: None,
                columns: None,
                filter: Some(Expr::And(
                    Box::new(Expr::Cmp(
                        Operand::Path(name("Artist")),
                        Cmp::Eq,
                        Operand::Value(Lit::Param(0))
                    )),
                    Box::new(Expr::Cmp(
                        Operand::Path(name("SongTitle")),
                        Cmp::Eq,
                        Operand::Value(Lit::Str("Hi".into()))
                    )),
                )),
            }
        );
        assert!(matches!(
            ok(r#"select a, b.c[2] from t."by-x" where x between 1 and 2"#),
            Statement::Select { index: Some(i), columns: Some(c), .. } if i == "by-x" && c.len() == 2
        ));
        assert_eq!(
            ok(
                "INSERT INTO t VALUE {'pk': ?, 'n': -1.5e3, 'l': [1, 'x'], 's': <<'a','b'>>, 'm': {}, 'b': true, 'z': null}"
            ),
            Statement::Insert {
                table: "t".into(),
                item: vec![
                    ("pk".into(), Lit::Param(0)),
                    ("n".into(), Lit::Num("-1.5e3".into())),
                    (
                        "l".into(),
                        Lit::List(vec![Lit::Num("1".into()), Lit::Str("x".into())])
                    ),
                    (
                        "s".into(),
                        Lit::Bag(vec![Lit::Str("a".into()), Lit::Str("b".into())])
                    ),
                    ("m".into(), Lit::Map(vec![])),
                    ("b".into(), Lit::Bool(true)),
                    ("z".into(), Lit::Null),
                ],
            }
        );
        let Statement::Update {
            set,
            remove,
            returning,
            ..
        } = ok(
            "UPDATE t SET a = a + 1, b = ? SET c = list_append(c, [1]) REMOVE d, e[0] WHERE pk = 'x' RETURNING ALL NEW *",
        )
        else {
            panic!()
        };
        assert_eq!(
            (set.len(), remove.len(), returning),
            (3, 2, Some(Returning::AllNew))
        );
        assert!(matches!(
            ok("DELETE FROM t WHERE pk = ? AND NOT (x IS MISSING);"),
            Statement::Delete { .. }
        ));
    }

    #[test]
    fn parameters_are_counted_in_order() {
        let p = parse("UPDATE t SET a = ? WHERE pk = ? AND sk IN [?, ?]").unwrap();
        assert_eq!(p.params, 4);
    }

    #[test]
    fn what_is_not_built_is_refused_by_name() {
        for (text, what) in [
            ("SELECT * FROM t ORDER BY a", "ORDER BY"),
            ("SELECT COUNT(*) FROM t", "functions in a SELECT list"),
            ("SELECT * FROM t WHERE EXISTS (SELECT * FROM u)", "EXISTS"),
            ("UPSERT INTO t VALUE {}", "UPSERT and REPLACE"),
            (
                "UPDATE t SET a = set_add(b, <<1>>) WHERE pk = 1",
                "set_add and set_delete on an attribute other than the one assigned",
            ),
        ] {
            assert_eq!(parse(text).unwrap_err(), Error::NotBuilt(what), "{text}");
        }
    }

    #[test]
    fn malformed_statements_are_refused() {
        for text in [
            "",
            "SELECT",
            "SELECT * FROM",
            "SELECT * FROM t WHERE",
            "SELECT * FROM t WHERE a",
            "SELECT * FROM t WHERE a = 'x",
            "INSERT INTO t VALUE 1",
            "INSERT INTO t VALUE {pk: 1}",
            "UPDATE t WHERE pk = 1",
            "DELETE FROM t",
            "DELETE FROM t WHERE pk = 1 RETURNING ALL NEW *",
            "SELECT * FROM t; SELECT * FROM t",
            "SELECT * FROM t WHERE a[x] = 1",
            "SELECT * FROM t WHERE a = <<>>",
            "SELECT * FROM t WHERE a = 1 #",
        ] {
            assert!(matches!(parse(text), Err(Error::Syntax(_))), "{text}");
        }
    }

    #[test]
    fn nesting_and_length_are_bounded() {
        let deep = format!(
            "SELECT * FROM t WHERE {}a = 1{}",
            "(".repeat(200),
            ")".repeat(200)
        );
        assert!(matches!(parse(&deep), Err(Error::Syntax(_))));
        let nots = format!("SELECT * FROM t WHERE {}a = 1", "NOT ".repeat(200));
        assert!(matches!(parse(&nots), Err(Error::Syntax(_))));
        let lists = format!(
            "INSERT INTO t VALUE {{'a': {}1{}}}",
            "[".repeat(200),
            "]".repeat(200)
        );
        assert!(matches!(parse(&lists), Err(Error::Syntax(_))));
        let long = format!("SELECT * FROM t WHERE a = '{}'", "x".repeat(9000));
        assert!(matches!(parse(&long), Err(Error::Syntax(_))));
    }

    /// The fuzz target's property, run on a fixed stream of mutations so it is part of every test run: any text is read or
    /// refused, never a panic.
    #[test]
    fn mutated_statements_never_panic() {
        let seeds = [
            "SELECT * FROM \"t\".\"i\" WHERE a = ? AND (b <> 'x' OR c BETWEEN 1 AND 2) AND d IN [1,2] AND begins_with(e, 'p')",
            "INSERT INTO t VALUE {'a': [1, {'b': <<'c'>>}], 'd': -2.5e-3}",
            "UPDATE t SET a = if_not_exists(a, 0) + 1 REMOVE b[3].c WHERE pk = ? RETURNING MODIFIED OLD *",
            "DELETE FROM t WHERE pk = 'x' AND attribute_type(a, 'S') AND size(b) > 3 AND c IS NOT NULL",
        ];
        let alphabet: Vec<char> =
            "SELECT*FROMWHEREAND OR NOT()[]{}<>=!?'\".,:-+;0123456789eabc_\u{e9}\n"
                .chars()
                .collect();
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut rand = |n: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            usize::try_from(state % (n as u64)).unwrap_or(0)
        };
        for _ in 0..20_000 {
            let mut chars: Vec<char> = seeds[rand(seeds.len())].chars().collect();
            for _ in 0..1 + rand(6) {
                let at = rand(chars.len() + 1);
                match rand(3) {
                    0 if at < chars.len() => {
                        chars.remove(at);
                    }
                    1 => chars.insert(at, alphabet[rand(alphabet.len())]),
                    _ if at < chars.len() => chars[at] = alphabet[rand(alphabet.len())],
                    _ => {}
                }
            }
            let text: String = chars.into_iter().collect();
            let _ = parse(&text);
        }
    }
}
