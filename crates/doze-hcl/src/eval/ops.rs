//! Operators (hclsyntax's `expression_ops.go`, with cty's stdlib operator functions).

use super::{Context, Evaluator, Out};
use crate::convert::convert;
use crate::diag::{Diagnostic, Diagnostics};
use crate::number::{ArithError, Number};
use crate::types::Type;
use crate::value::Value;
use hcl_edit::expr::{BinaryOp, BinaryOperator, UnaryOp, UnaryOperator};
use std::cmp::Ordering;

/// What an operator's operands are converted to, and what it returns.
fn operand_type(op: BinaryOperator) -> (Type, Type) {
    use BinaryOperator as B;
    match op {
        B::Eq | B::NotEq => (Type::Dynamic, Type::Bool),
        B::And | B::Or => (Type::Bool, Type::Bool),
        B::Less | B::LessEq | B::Greater | B::GreaterEq => (Type::Number, Type::Bool),
        B::Plus | B::Minus | B::Mul | B::Div | B::Mod => (Type::Number, Type::Number),
    }
}

impl Evaluator<'_> {
    pub(super) fn unary(&self, u: &UnaryOp, ctx: &Context<'_>) -> Out {
        let (param, ret) = match u.operator.value() {
            UnaryOperator::Neg => (Type::Number, Type::Number),
            UnaryOperator::Not => (Type::Bool, Type::Bool),
        };
        let (given, mut diags) = self.eval(&u.expr, ctx);
        let val = match convert(&given, &param) {
            Ok(v) => Some(v),
            Err(e) => {
                let d = Diagnostic::error(
                    "Invalid operand",
                    format!("Unsuitable value for unary operand: {e}."),
                    self.range(&u.expr),
                );
                diags.push(d.with_context(self.range(u)));
                None
            }
        };
        let Some(val) = val.filter(|_| !diags.has_errors()) else {
            return (Value::Unknown(ret), diags);
        };
        // Negate and Not take no nulls, and return an unknown for an unknown.
        let out = match (&val, u.operator.value()) {
            (Value::Null(_), _) => Err("argument must not be null".to_owned()),
            (Value::Unknown(_), _) => Ok(Value::Unknown(ret.clone())),
            (Value::Number(n), UnaryOperator::Neg) => Ok(Value::Number(n.neg())),
            (Value::Bool(b), UnaryOperator::Not) => Ok(Value::Bool(!b)),
            _ => Ok(Value::Unknown(ret.clone())),
        };
        self.finish(out, ret, self.range(u), diags)
    }

    fn finish(
        &self,
        out: Result<Value, String>,
        ret: Type,
        at: crate::diag::Range,
        mut diags: Diagnostics,
    ) -> Out {
        match out {
            Ok(v) => (v, diags),
            Err(e) => {
                diags.push(Diagnostic::error(
                    "Operation failed",
                    format!("Error during operation: {e}."),
                    at,
                ));
                (Value::Unknown(ret), diags)
            }
        }
    }

    pub(super) fn binary(&self, b: &BinaryOp, ctx: &Context<'_>) -> Out {
        let op = *b.operator.value();
        let (param, ret) = operand_type(op);
        let mut diags = Diagnostics::default();
        let (lhs_given, lhs_diags) = self.eval(&b.lhs_expr, ctx);
        let lhs = self.operand(&lhs_given, &param, "left", b, &mut diags);
        let (rhs_given, rhs_diags) = self.eval(&b.rhs_expr, ctx);
        let rhs = self.operand(&rhs_given, &param, "right", b, &mut diags);
        let (Some(lhs), Some(rhs)) = (lhs, rhs) else {
            diags.extend(lhs_diags);
            diags.extend(rhs_diags);
            return (Value::Unknown(ret), diags);
        };
        if matches!(op, BinaryOperator::And | BinaryOperator::Or)
            && let Some((v, d)) = short_circuit(op, &lhs, &rhs, &lhs_diags, &rhs_diags)
        {
            return (v, d);
        }
        diags.extend(lhs_diags);
        diags.extend(rhs_diags);
        if diags.has_errors() {
            return (Value::Unknown(ret), diags);
        }
        let out = apply(op, &lhs, &rhs, &ret);
        self.finish(out, ret, self.range(b), diags)
    }

    fn operand(
        &self,
        given: &Value,
        param: &Type,
        side: &str,
        b: &BinaryOp,
        diags: &mut Diagnostics,
    ) -> Option<Value> {
        let expr = if side == "left" {
            &b.lhs_expr
        } else {
            &b.rhs_expr
        };
        match convert(given, param) {
            Ok(v) => Some(v),
            Err(e) => {
                let d = Diagnostic::error(
                    "Invalid operand",
                    format!("Unsuitable value for {side} operand: {e}."),
                    self.range(expr),
                );
                diags.push(d.with_context(self.range(b)));
                None
            }
        }
    }
}

/// `&&` and `||` decide from one side when they can (hclsyntax's `ShortCircuit`).
fn short_circuit(
    op: BinaryOperator,
    lhs: &Value,
    rhs: &Value,
    ld: &Diagnostics,
    rd: &Diagnostics,
) -> Option<Out> {
    let decides = op == BinaryOperator::Or;
    let unknown = || Value::Unknown(Type::Bool);
    // cty reads a null bool as not true and as false (`True()` is false, `False()` is `!True()`), so
    // `null || true` is true and `true && null` is false.
    let is = |v: &Value, b: bool| {
        if b {
            v.as_bool() == Some(true)
        } else {
            v.is_null() || v.as_bool() == Some(false)
        }
    };
    match (lhs.is_known(), rhs.is_known()) {
        (false, false) => (!ld.has_errors()).then(|| (unknown(), ld.clone())),
        _ if is(lhs, decides) => Some((Value::Bool(decides), ld.clone())),
        _ if is(rhs, decides) => Some((Value::Bool(decides), rd.clone())),
        (false, true) if is(rhs, !decides) => Some((unknown(), ld.clone())),
        (true, false) if is(lhs, !decides) => Some((unknown(), rd.clone())),
        _ => None,
    }
}

/// The operator's function, with cty's rules: no nulls (except `==` and `!=`), and an unknown in an
/// arithmetic or logical operand makes the result unknown.
fn apply(op: BinaryOperator, lhs: &Value, rhs: &Value, ret: &Type) -> Result<Value, String> {
    use BinaryOperator as B;
    match op {
        B::Eq => return Ok(lhs.equals(rhs)),
        B::NotEq => {
            return Ok(match lhs.equals(rhs) {
                Value::Bool(b) => Value::Bool(!b),
                other => other,
            });
        }
        _ => {}
    }
    if lhs.is_null() || rhs.is_null() {
        return Err("argument must not be null".to_owned());
    }
    let (Value::Number(a), Value::Number(b)) = (lhs, rhs) else {
        return Ok(match (op, lhs, rhs) {
            (B::And, Value::Bool(a), Value::Bool(b)) => Value::Bool(*a && *b),
            (B::Or, Value::Bool(a), Value::Bool(b)) => Value::Bool(*a || *b),
            _ => Value::Unknown(ret.clone()),
        });
    };
    let arith = |r: Result<Number, ArithError>| r.map(Value::Number).map_err(|e| e.to_string());
    match op {
        B::Plus => arith(a.add(b)),
        B::Minus => arith(a.sub(b)),
        B::Mul => arith(a.mul(b)),
        B::Div => arith(a.div(b)),
        B::Mod => arith(a.modulo(b)),
        B::Less => Ok(Value::Bool(a.cmp_value(b) == Ordering::Less)),
        B::LessEq => Ok(Value::Bool(a.cmp_value(b) != Ordering::Greater)),
        B::Greater => Ok(Value::Bool(a.cmp_value(b) == Ordering::Greater)),
        B::GreaterEq => Ok(Value::Bool(a.cmp_value(b) != Ordering::Less)),
        _ => Ok(Value::Unknown(ret.clone())),
    }
}
