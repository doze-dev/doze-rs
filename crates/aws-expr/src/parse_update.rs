//! Update and projection expressions.
//!
//! ```text
//! update := (set | remove | add | delete)+          each clause at most once
//! set    := 'SET' path '=' operand (('+' | '-') operand)? (',' ...)*
//! remove := 'REMOVE' path (',' path)*
//! add    := 'ADD' path :value (',' path :value)*
//! delete := 'DELETE' path :value (',' path :value)*
//! ```

use crate::Invalid;
use crate::ast::{Operand, Path, SetValue, Update};
use crate::lex::Tok;
use crate::parse::Parser;
use crate::scope::{Kind, Scope};
use crate::value::Value;

// fuzz:parser fuzz_expression
/// Reads an update expression.
///
/// # Errors
/// AWS's refusal, worded as it words it.
pub fn parse_update(text: &str, scope: &mut Scope<'_>) -> Result<Update, Invalid> {
    let mut p = Parser::new(text, Kind::Update, scope)?;
    let mut update = Update::default();
    let mut seen = [false; 4];
    if p.peek() == Tok::Eof {
        return Err(p.kind.invalid("The expression can not be empty;"));
    }
    while p.peek() != Tok::Eof {
        let clause = clause_of(&p).ok_or_else(|| p.syntax())?;
        if std::mem::replace(seen.get_mut(clause).ok_or_else(|| p.syntax())?, true) {
            let word = p.peek_text().to_ascii_uppercase();
            return Err(p.kind.invalid(&format!(
                "The \"{word}\" section can only be used once in an update expression;"
            )));
        }
        p.bump();
        match clause {
            0 => p.set_clause(&mut update)?,
            1 => update.remove = p.list(Parser::path)?,
            2 => update.add = p.list(Parser::path_and_value)?,
            _ => update.delete = p.list(Parser::path_and_value)?,
        }
    }
    p.finish()?;
    overlaps(&update, Kind::Update)?;
    Ok(update)
}

// fuzz:parser fuzz_expression
/// Reads a projection expression: a comma-separated list of document paths.
///
/// # Errors
/// AWS's refusal, worded as it words it.
pub fn parse_projection(text: &str, scope: &mut Scope<'_>) -> Result<Vec<Path>, Invalid> {
    let mut p = Parser::new(text, Kind::Projection, scope)?;
    let paths = p.list(Parser::path)?;
    p.finish()?;
    let tagged: Vec<&Path> = paths.iter().collect();
    first_overlap(&tagged, Kind::Projection)?;
    Ok(paths)
}

fn clause_of(p: &Parser<'_, '_>) -> Option<usize> {
    ["SET", "REMOVE", "ADD", "DELETE"]
        .iter()
        .position(|w| p.at_word(w))
}

impl Parser<'_, '_> {
    /// `item (',' item)*`.
    fn list<T>(
        &mut self,
        one: impl Fn(&mut Self) -> Result<T, Invalid>,
    ) -> Result<Vec<T>, Invalid> {
        let mut out = vec![one(self)?];
        while self.eat(Tok::Comma) {
            out.push(one(self)?);
        }
        Ok(out)
    }

    fn path_and_value(&mut self) -> Result<(Path, Value), Invalid> {
        let path = self.path()?;
        let reference = self.peek_text();
        self.expect(Tok::ValueRef)?;
        Ok((path, self.value_ref(reference)))
    }

    fn set_clause(&mut self, update: &mut Update) -> Result<(), Invalid> {
        update.set = self.list(Parser::assignment)?;
        Ok(())
    }

    fn assignment(&mut self) -> Result<(Path, SetValue), Invalid> {
        let path = self.path()?;
        self.expect(Tok::Eq)?;
        let first = self.operand()?;
        let value = match self.peek() {
            Tok::Plus => {
                self.bump();
                SetValue::Plus(first, self.operand()?)
            }
            Tok::Minus => {
                self.bump();
                SetValue::Minus(first, self.operand()?)
            }
            _ => SetValue::Operand(first),
        };
        Ok((path, value))
    }
}

/// Every path an update touches, in the order of its clauses.
fn update_paths(u: &Update) -> Vec<&Path> {
    let mut all: Vec<&Path> = u.set.iter().map(|(p, _)| p).collect();
    all.extend(&u.remove);
    all.extend(u.add.iter().map(|(p, _)| p));
    all.extend(u.delete.iter().map(|(p, _)| p));
    all
}

fn overlaps(u: &Update, kind: Kind) -> Result<(), Invalid> {
    first_overlap(&update_paths(u), kind)
}

/// Two paths that are the same, or one inside the other, may not both appear. `(memory)` for both texts: AWS says "overlap" for
/// the same path and "conflict" for one inside the other.
fn first_overlap(paths: &[&Path], kind: Kind) -> Result<(), Invalid> {
    for (i, a) in paths.iter().enumerate() {
        for b in paths.iter().skip(i + 1) {
            if a.overlaps(b) {
                let word = if a == b { "overlap" } else { "conflict" };
                return Err(kind.invalid(&format!(
                    "Two document paths {word} with each other; must remove or rewrite one of these paths; path one: {}, path two: {}",
                    a.shown(),
                    b.shown()
                )));
            }
        }
    }
    Ok(())
}

/// The operands a `SET` right-hand side uses, for the evaluator's convenience.
impl SetValue {
    /// The one or two operands.
    #[must_use]
    pub fn operands(&self) -> Vec<&Operand> {
        match self {
            Self::Operand(a) => vec![a],
            Self::Plus(a, b) | Self::Minus(a, b) => vec![a, b],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{NameMap, ValueMap};
    use crate::number::Number;

    fn values() -> ValueMap {
        [
            (":one".to_owned(), Value::N(Number::from_i64(1))),
            (":s".to_owned(), Value::Ss(["x".to_owned()].into())),
        ]
        .into()
    }

    fn update(text: &str) -> Result<Update, Invalid> {
        let (names, values): (NameMap, ValueMap) =
            ([("#n".to_owned(), "n".to_owned())].into(), values());
        parse_update(text, &mut Scope::new(&names, &values))
    }

    #[test]
    fn the_recorded_update_parses() {
        let u = update(
            "SET #n = #n + :one, tags = if_not_exists(tags, :one) ADD seen :s REMOVE gone, a.b[1]",
        )
        .unwrap();
        assert_eq!(
            (u.set.len(), u.add.len(), u.remove.len(), u.delete.len()),
            (2, 1, 2, 0)
        );
    }

    #[test]
    fn refusals() {
        let err = |t: &str| update(t).unwrap_err().0;
        assert_eq!(
            err("SET"),
            "1 validation error detected: Invalid UpdateExpression: Syntax error; token: \"<EOF>\", near: \"SET\""
        );
        assert!(err("SET name = :one").contains("reserved keyword; reserved keyword: name"));
        assert!(err("SET n = :missing").contains(
            "attribute value used in expression is not defined; attribute value: :missing"
        ));
        assert!(
            err("SET a = :one SET b = :one").contains("The \"SET\" section can only be used once")
        );
        assert!(err("SET a = :one REMOVE a").contains("Two document paths overlap"));
        assert!(err("SET a.b = :one, a = :one").contains("Two document paths conflict"));
        assert!(err("").contains("can not be empty"));
        assert!(err("GET a").contains("Syntax error"));
    }

    #[test]
    fn projection() {
        let (names, values) = (NameMap::new(), ValueMap::new());
        let ok = parse_projection("a, b.c, d[0]", &mut Scope::new(&names, &values)).unwrap();
        assert_eq!(ok.len(), 3);
        let e = parse_projection("a, a.b", &mut Scope::new(&names, &values)).unwrap_err();
        assert!(e.0.contains("Invalid ProjectionExpression: Two document paths conflict"));
    }
}
