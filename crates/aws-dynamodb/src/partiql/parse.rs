//! The PartiQL that DynamoDB takes, read into a statement: `SELECT`, `INSERT`, `UPDATE` and `DELETE`, one at a time.
//!
//! Bounded: the model caps a statement at 8,192 characters, and nesting (parentheses, `NOT`, lists and maps) is cut off at
//! [`MAX_DEPTH`], so no input makes the parser recurse without end. What is recognised but not built (`ORDER BY`, `EXISTS`, functions
//! in a select list) is refused by name ([`Error::NotBuilt`]), never read as something else. Every message is `(memory)`.

mod ast;
mod expr;
mod lex;
#[cfg(test)]
mod tests;

pub(crate) use ast::*;
use lex::{Tok, describe, lex};

/// How deep parentheses, `NOT`s, lists and maps may nest.
const MAX_DEPTH: usize = 32;
/// The model's limit on a statement's length.
const MAX_LEN: usize = 8192;

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
}
