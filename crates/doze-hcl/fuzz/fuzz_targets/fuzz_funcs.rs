//! Every function of doze.hcl's table called with arguments built from the fuzz bytes: the first
//! byte picks the function, then each argument (separated by 0xFF) is either one of a set of
//! values of every kind (nulls, unknowns, collections, numbers at the edges) or the bytes as a
//! string literal (so format strings, patterns, JSON and CSV text are fuzzed directly), or as raw
//! expression text. A call never panics and never runs away: widths, `indent`, `range`, JSON and
//! regex nesting and regex size are all bounded.
#![no_main]

use doze_hcl::eval::{Context, eval_attribute};
use doze_hcl::funcs::doze_functions_with_env;
use doze_hcl::{Number, Type, Value};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;
use std::sync::Arc;

const ATOMS: &[&str] = &[
    "null",
    "var.later",
    "var.any",
    "var.none",
    "var.tags",
    "var.set",
    "var.env",
    "var.users",
    "0",
    "-1",
    "1.5",
    "1e400",
    "-1e-400",
    "1/0",
    "18446744073709551616",
    "1/3",
    "true",
    "\"\"",
    "\"%v\"",
    "\"a,b\\n1,2\"",
    "[]",
    "{}",
    "[1, \"a\", null]",
    "[var.later, 1]",
    "{a = 1, b = \"x\"}",
    "{env = {A = \"1\"}, name = \"db\"}",
    "tolist([])",
    "toset([var.later, \"a\"])",
    "tomap({a = var.later})",
    "[[1], [2, [3]]]",
    "\"${var.later}\"",
    "\"x${var.later}\"",
];

fn variables() -> BTreeMap<String, Value> {
    let s = Value::string;
    let n = |i| Value::Number(Number::from_i64(i));
    let obj = |pairs: Vec<(&str, Value)>| {
        Value::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    };
    let var = obj(vec![
        (
            "tags",
            Value::List(Type::String, vec![s("a"), s("b"), s("c")]),
        ),
        ("set", Value::Set(Type::String, vec![s("a"), s("b")])),
        (
            "env",
            Value::Map(
                Type::String,
                [("A".to_owned(), s("1")), ("B".to_owned(), s("2"))].into(),
            ),
        ),
        (
            "users",
            Value::Tuple(vec![
                obj(vec![("name", s("ann")), ("age", n(31))]),
                obj(vec![("name", s("bo")), ("age", n(27))]),
            ]),
        ),
        ("none", Value::Null(Type::String)),
        ("later", Value::unknown(Type::String)),
        ("any", Value::DYNAMIC),
    ]);
    [("var".to_owned(), var)].into()
}

/// An HCL string literal holding `bytes` (lossily decoded), with `"`, `\`, `${` and `%{` escaped.
fn quoted(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '$' => out.push_str("$$"),
            '%' => out.push_str("%%"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn argument(bytes: &[u8]) -> String {
    match bytes.split_first() {
        Some((b, rest)) if *b < 0x40 => {
            let atom = ATOMS
                .get(usize::from(*b) % ATOMS.len())
                .copied()
                .unwrap_or("null");
            if rest.is_empty() {
                atom.to_owned()
            } else {
                format!("{atom}{}", if rest.len() % 2 == 0 { "..." } else { "" })
            }
        }
        Some((b, rest)) if *b < 0x50 => String::from_utf8_lossy(rest).into_owned(),
        Some((_, rest)) => quoted(rest),
        None => "\"\"".to_owned(),
    }
}

fuzz_target!(|data: &[u8]| {
    let funcs = doze_functions_with_env(Arc::new(|name: &str| {
        (name == "PATH").then(|| "/usr/bin:/bin".to_owned())
    }));
    let Some((pick, rest)) = data.split_first() else {
        return;
    };
    let names: Vec<&String> = funcs.keys().collect();
    let Some(name) = names.get(usize::from(*pick) % names.len()) else {
        return;
    };
    let args: Vec<String> = rest.split(|b| *b == 0xFF).take(6).map(argument).collect();
    let expr = format!("x = {name}({})\n", args.join(", "));
    let ctx = Context::new(variables(), Some(&funcs));
    if let Ok((v, _diags)) = eval_attribute(&expr, "x", &ctx) {
        let _ = v.ty().to_json();
    }
});
