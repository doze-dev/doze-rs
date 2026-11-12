//! Condition and filter expressions.
//!
//! ```text
//! cond    := and ('OR' and)*
//! and     := not ('AND' not)*
//! not     := 'NOT' not | primary
//! primary := '(' cond ')' | function | operand (cmp operand | 'BETWEEN' operand 'AND' operand | 'IN' '(' operand, ... ')')
//! ```

use crate::Invalid;
use crate::ast::{Cmp, Cond, Func, Operand};
use crate::lex::Tok;
use crate::parse::Parser;
use crate::scope::{Kind, Scope};
use crate::value::Value;

/// The most operands `IN` takes. `(memory)`: the developer guide gives 100.
const MAX_IN: usize = 100;

// fuzz:parser fuzz_expression
/// Reads a condition or filter expression.
///
/// # Errors
/// AWS's refusal, worded as it words it.
pub fn parse_condition(text: &str, kind: Kind, scope: &mut Scope<'_>) -> Result<Cond, Invalid> {
    let mut p = Parser::new(text, kind, scope)?;
    let cond = p.or()?;
    p.finish()?;
    Ok(cond)
}

fn boolean_function(word: &str) -> Option<Func> {
    Some(match word {
        "attribute_exists" => Func::AttributeExists,
        "attribute_not_exists" => Func::AttributeNotExists,
        "attribute_type" => Func::AttributeType,
        "begins_with" => Func::BeginsWith,
        "contains" => Func::Contains,
        _ => return None,
    })
}

impl Parser<'_, '_> {
    fn or(&mut self) -> Result<Cond, Invalid> {
        let mut left = self.and()?;
        while self.at_word("OR") {
            self.bump();
            left = Cond::Or(Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Cond, Invalid> {
        let mut left = self.not()?;
        while self.at_word("AND") {
            self.bump();
            left = Cond::And(Box::new(left), Box::new(self.not()?));
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Cond, Invalid> {
        if self.at_word("NOT") {
            self.bump();
            return Ok(Cond::Not(Box::new(self.not()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Cond, Invalid> {
        if self.eat(Tok::LParen) {
            let inner = self.or()?;
            self.expect(Tok::RParen)?;
            return Ok(inner);
        }
        if self.peek() == Tok::Word
            && self.nth(1) == Tok::LParen
            && let Some(func) = boolean_function(self.peek_text())
        {
            return self.function(func);
        }
        let left = self.operand()?;
        self.after_operand(left)
    }

    fn after_operand(&mut self, left: Operand) -> Result<Cond, Invalid> {
        let cmp = match self.peek() {
            Tok::Eq => Cmp::Eq,
            Tok::Ne => Cmp::Ne,
            Tok::Lt => Cmp::Lt,
            Tok::Le => Cmp::Le,
            Tok::Gt => Cmp::Gt,
            Tok::Ge => Cmp::Ge,
            _ if self.at_word("BETWEEN") => return self.between(left),
            _ if self.at_word("IN") => return self.in_list(left),
            _ => return Err(self.syntax()),
        };
        self.bump();
        Ok(Cond::Cmp(cmp, left, self.operand()?))
    }

    fn between(&mut self, left: Operand) -> Result<Cond, Invalid> {
        self.bump();
        let lo = self.operand()?;
        if !self.at_word("AND") {
            return Err(self.syntax());
        }
        self.bump();
        let hi = self.operand()?;
        if let (Operand::Value(a), Operand::Value(b)) = (&lo, &hi) {
            self.check_bounds(a, b);
        }
        Ok(Cond::Between(left, lo, hi))
    }

    /// Literal bounds are checked when the expression is read, as AWS does. `(memory)` for both texts.
    fn check_bounds(&mut self, lo: &Value, hi: &Value) {
        let shown = |v: &Value| format!("AttributeValue: {}", brief(v));
        let what = if lo.type_name() != hi.type_name() {
            "The BETWEEN operator requires same data type for lower and upper bounds"
        } else if lo.compare(hi).is_some_and(|o| o.is_gt()) {
            "The BETWEEN operator requires upper bound to be greater than or equal to lower bound"
        } else {
            return;
        };
        let msg = format!(
            "{what}; lower bound operand: {}, upper bound operand: {}",
            shown(lo),
            shown(hi)
        );
        self.defer(self.kind.invalid(&msg));
    }

    fn in_list(&mut self, left: Operand) -> Result<Cond, Invalid> {
        self.bump();
        self.expect(Tok::LParen)?;
        let mut items = vec![self.operand()?];
        while self.eat(Tok::Comma) {
            items.push(self.operand()?);
        }
        self.expect(Tok::RParen)?;
        if items.len() > MAX_IN {
            let msg = format!(
                "Too many operands for the IN operator; max: {MAX_IN}, actual: {}",
                items.len()
            );
            self.defer(self.kind.invalid(&msg));
        }
        Ok(Cond::In(left, items))
    }

    fn function(&mut self, func: Func) -> Result<Cond, Invalid> {
        self.bump();
        self.expect(Tok::LParen)?;
        let mut args = vec![self.operand()?];
        while self.eat(Tok::Comma) {
            args.push(self.operand()?);
        }
        self.expect(Tok::RParen)?;
        let wanted = match func {
            Func::AttributeExists | Func::AttributeNotExists => 1,
            _ => 2,
        };
        if args.len() != wanted {
            let msg = format!(
                "Incorrect number of operands for operator or function; operator or function: {}, number of operands: {}",
                func.name(),
                args.len()
            );
            return Err(self.kind.invalid(&msg));
        }
        self.check_function(func, &args)?;
        Ok(Cond::Func(func, args))
    }

    /// The operand types a function can be told from the text alone.
    fn check_function(&mut self, func: Func, args: &[Operand]) -> Result<(), Invalid> {
        let wrong = |me: &Self, v: &Value| {
            me.kind.invalid(&format!(
                "Incorrect operand type for operator or function; operator or function: {}, operand type: {}",
                func.name(),
                v.type_name()
            ))
        };
        let (first, second) = (args.first(), args.get(1));
        if matches!(
            func,
            Func::AttributeExists | Func::AttributeNotExists | Func::AttributeType
        ) && let Some(Operand::Value(v)) = first
        {
            return Err(wrong(self, v));
        }
        match (func, second) {
            (Func::BeginsWith, Some(Operand::Value(v)))
                if !matches!(v, Value::S(_) | Value::B(_)) =>
            {
                Err(wrong(self, v))
            }
            (Func::Contains, Some(Operand::Value(v)))
                if !matches!(v, Value::S(_) | Value::N(_) | Value::B(_)) =>
            {
                Err(wrong(self, v))
            }
            (Func::AttributeType, Some(Operand::Value(v))) => self.check_type_name(v),
            _ => Ok(()),
        }
    }

    fn check_type_name(&mut self, v: &Value) -> Result<(), Invalid> {
        let known = ["S", "SS", "N", "NS", "B", "BS", "BOOL", "NULL", "L", "M"];
        match v {
            Value::S(name) if known.contains(&name.as_str()) => Ok(()),
            Value::S(name) => Err(self.kind.invalid(&format!("Invalid attribute type name found; type: {name}, operator: attribute_type"))),
            other => Err(self.kind.invalid(&format!(
                "Incorrect operand type for operator or function; operator or function: attribute_type, operand type: {}",
                other.type_name()
            ))),
        }
    }
}

/// The value as Java prints an `AttributeValue` with one field set, which is how AWS words it: `{N:5}`.
fn brief(v: &Value) -> String {
    let body = match v {
        Value::S(s) => s.clone(),
        Value::N(n) => n.to_string(),
        other => other.type_name().to_owned(),
    };
    format!("{{{}:{body}}}", v.type_name())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::ast::{NameMap, ValueMap};

    fn parse(text: &str, values: &[(&str, Value)]) -> Result<Cond, Invalid> {
        let names: NameMap = [("#n".to_owned(), "name".to_owned())].into();
        let values: ValueMap = values
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect::<BTreeMap<_, _>>();
        parse_condition(text, Kind::Condition, &mut Scope::new(&names, &values))
    }

    fn s(x: &str) -> Value {
        Value::S(x.into())
    }

    #[test]
    fn precedence_is_not_then_and_then_or() {
        let c = parse("a = :x OR NOT b = :x AND c = :x", &[(":x", s("1"))]).unwrap();
        let Cond::Or(_, right) = c else { panic!("or") };
        let Cond::And(left, _) = *right else {
            panic!("and")
        };
        assert!(matches!(*left, Cond::Not(_)));
    }

    #[test]
    fn functions_between_and_in() {
        let v = [
            (":a", Value::N(crate::Number::from_i64(1))),
            (":b", Value::N(crate::Number::from_i64(2))),
            (":s", s("p")),
        ];
        assert!(parse("a BETWEEN :a AND :b", &v).is_ok());
        assert!(parse("a IN (:a, :b)", &v).is_ok());
        assert!(
            parse(
                "begins_with(a, :s) AND attribute_exists(b.c[1]) AND size(a) > :a",
                &v
            )
            .is_ok()
        );
        assert!(parse("(a = :a)", &v).is_ok());
        assert!(parse("#n = :a", &v).is_ok());
    }

    #[test]
    fn refusals_use_aws_words() {
        let v = [
            (":a", Value::N(crate::Number::from_i64(1))),
            (":b", Value::N(crate::Number::from_i64(0))),
        ];
        let err = |t: &str| parse(t, &v).unwrap_err().0;
        assert_eq!(
            err("a ="),
            "Invalid ConditionExpression: Syntax error; token: \"<EOF>\", near: \"=\""
        );
        assert_eq!(
            err("a = = :a"),
            "Invalid ConditionExpression: Syntax error; token: \"=\", near: \"= =\""
        );
        assert!(err("name = :a").contains("reserved keyword; reserved keyword: name"));
        assert!(
            err("a = :zz").contains(
                "attribute value used in expression is not defined; attribute value: :zz"
            )
        );
        assert!(err("#zz = :a").contains(
            "attribute name used in the document path is not defined; attribute name: #zz"
        ));
        assert!(err("foo(a) = :a").contains("Invalid function name; function: foo"));
        assert!(err("begins_with(a)").contains("Incorrect number of operands"));
        assert!(err("begins_with(a, :a)").contains("Incorrect operand type for operator or function; operator or function: begins_with, operand type: N"));
        assert!(err("a BETWEEN :a AND :b").contains("upper bound to be greater"));
    }
}
