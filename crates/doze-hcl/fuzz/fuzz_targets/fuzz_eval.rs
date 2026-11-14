//! Any text that parses: every attribute evaluates to a value and diagnostics, never panics (the
//! `5 % 0` class), with variables of every kind in scope, including nulls and unknowns.
#![no_main]

use doze_hcl::eval::{Context, Evaluator};
use doze_hcl::function::Functions;
use doze_hcl::syntax::{Source, parse_body};
use doze_hcl::{Number, Type, Value};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;

fn variables() -> BTreeMap<String, Value> {
    let s = Value::string;
    let mut var = BTreeMap::new();
    var.insert("name".to_owned(), s("shop"));
    var.insert("port".to_owned(), Value::Number(Number::from_i64(8080)));
    var.insert("tags".to_owned(), Value::List(Type::String, vec![s("a"), s("b")]));
    var.insert("none".to_owned(), Value::Null(Type::String));
    var.insert("later".to_owned(), Value::Unknown(Type::String));
    var.insert("any".to_owned(), Value::DYNAMIC);
    var.insert("set".to_owned(), Value::Set(Type::Number, vec![Value::Number(Number::from_i64(1))]));
    let mut vars = BTreeMap::new();
    vars.insert("var".to_owned(), Value::Object(var));
    vars
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let src = Source::new(text);
    let Ok(body) = parse_body(&src) else { return };
    let funcs = Functions::new();
    let ctx = Context::new(variables(), Some(&funcs));
    let ev = Evaluator::new(&src);
    for attr in body.attributes() {
        let (v, _diags) = ev.eval(&attr.value, &ctx);
        let _ = v.ty().to_json();
    }
    for block in body.blocks() {
        for attr in block.body.attributes() {
            let _ = ev.eval(&attr.value, &ctx);
        }
    }
});
