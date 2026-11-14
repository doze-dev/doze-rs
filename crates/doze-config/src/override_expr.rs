//! A `--var` or `DOZE_VAR_<name>` value for a collection type, read as one HCL expression (Go's
//! `hclsyntax.ParseExpression` in a file named `<override>`), with Go's error text.

use doze_hcl::Value;
use doze_hcl::diag::{Diagnostics, Pos, Range};
use doze_hcl::eval::{Context, Evaluator};
use doze_hcl::syntax::{Source, parse_body};
use hcl_edit::Span;

/// The expression did not parse, or did not evaluate: Go's `Diagnostics.Error()` text.
#[derive(Debug)]
pub(crate) enum ExprError {
    Parse(String),
    Eval(String),
}

/// The text is parsed as the value of `x = `; positions are moved back onto the text itself.
const LEAD: u32 = 4;

pub(crate) fn parse(text: &str) -> Result<Value, ExprError> {
    let src = Source::new(&format!("x = {text}\n"));
    let body = parse_body(&src).map_err(|ds| ExprError::Parse(go_text(&ds)))?;
    let mut items = body
        .attributes()
        .map(|a| a.key.span())
        .chain(body.blocks().map(|b| b.ident.span()));
    let _first = items.next();
    if let Some(next) = items.next() {
        let at = shift(next.map(|s| src.range(s)).unwrap_or_default());
        return Err(ExprError::Parse(format!(
            "{}: Extra characters after expression; An expression was successfully parsed, but extra characters were found after it.",
            place(at)
        )));
    }
    let Some(attr) = body.get_attribute("x") else {
        return Err(ExprError::Parse(
            "<override>:1,1-1: Missing expression; Expected the start of an expression.".into(),
        ));
    };
    let (v, ds) = Evaluator::new(&src).eval(&attr.value, &Context::default());
    if ds.has_errors() {
        return Err(ExprError::Eval(go_text(&ds)));
    }
    Ok(v)
}

fn shift_pos(p: Pos) -> Pos {
    if p.line == 1 {
        Pos {
            line: 1,
            column: p.column.saturating_sub(LEAD).max(1),
            byte: p.byte.saturating_sub(LEAD),
        }
    } else {
        Pos {
            byte: p.byte.saturating_sub(LEAD),
            ..p
        }
    }
}

fn shift(r: Range) -> Range {
    Range {
        start: shift_pos(r.start),
        end: shift_pos(r.end),
    }
}

/// Go's `hcl.Range.String()` for `<override>`.
fn place(r: Range) -> String {
    if r.start.line == r.end.line {
        format!(
            "<override>:{},{}-{}",
            r.start.line, r.start.column, r.end.column
        )
    } else {
        format!(
            "<override>:{},{}-{},{}",
            r.start.line, r.start.column, r.end.line, r.end.column
        )
    }
}

/// Go's `hcl.Diagnostics.Error()`: the first, and how many others.
fn go_text(ds: &Diagnostics) -> String {
    let one = |d: &doze_hcl::diag::Diagnostic| {
        let at = d
            .subject
            .map_or_else(|| "<nil>".to_owned(), |r| place(shift(r)));
        format!("{at}: {}; {}", d.summary, d.detail)
    };
    match ds.0.as_slice() {
        [] => "no diagnostics".into(),
        [d] => one(d),
        [d, rest @ ..] => format!("{}, and {} other diagnostic(s)", one(d), rest.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_one_expression_with_go_positions() {
        assert!(matches!(parse("[\"a\", \"b\"]"), Ok(Value::Tuple(_))));
        let Err(ExprError::Parse(e)) = parse("1\ny = 2") else {
            panic!("extra characters are refused");
        };
        assert!(
            e.starts_with("<override>:2,1-2: Extra characters after expression"),
            "{e}"
        );
        let Err(ExprError::Eval(e)) = parse("foo") else {
            panic!("a variable is refused");
        };
        assert!(
            e.starts_with("<override>:1,1-4: Variables not allowed"),
            "{e}"
        );
    }
}
