//! What the oracle tests share: Go's evaluation context and value encoding (tools/oracle-dump).
#![allow(dead_code)]

use doze_hcl::{Number, Type, Value};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;

fn n(s: &str) -> Value {
    Value::Number(Number::parse(s).unwrap())
}

fn obj(pairs: &[(&str, Value)]) -> Value {
    Value::Object(
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
    )
}

/// The oracle's context (tools/oracle-dump/eval.go `testContext`).
pub(crate) fn variables() -> BTreeMap<String, Value> {
    let s = Value::string;
    let tags = Value::list(Type::String, vec![s("a"), s("b"), s("c")]).unwrap();
    let env = Value::map(
        Type::String,
        [("A", s("1")), ("B", s("2"))]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
    )
    .unwrap();
    let set = Value::set(Type::String, vec![s("b"), s("a")]).unwrap();
    let users = Value::Tuple(vec![
        obj(&[("name", s("ann")), ("age", n("31"))]),
        obj(&[("name", s("bo")), ("age", n("27"))]),
    ]);
    let var = obj(&[
        ("name", s("shop")),
        ("port", n("8080")),
        ("tags", tags),
        ("env", env),
        ("flag", Value::Bool(true)),
        ("none", Value::Null(Type::String)),
        ("set", set),
        ("users", users),
        ("later", Value::Unknown(Type::String)),
        ("any", Value::DYNAMIC),
    ]);
    let local = obj(&[("base", s("/srv")), ("n", n("3"))]);
    [("var".to_owned(), var), ("local".to_owned(), local)]
        .into_iter()
        .collect()
}

pub(crate) fn canon(v: &Value) -> Json {
    match v {
        Value::Unknown(_) => json!({"unknown": true}),
        Value::Null(_) => Json::Null,
        Value::String(s) => json!(s),
        Value::Bool(b) => json!(b),
        Value::Number(n) => json!({"num": n.text()}),
        Value::List(_, es) | Value::Set(_, es) | Value::Tuple(es) => {
            Json::Array(es.iter().map(canon).collect())
        }
        Value::Map(_, m) | Value::Object(m) => {
            json!({"obj": m.iter().map(|(k, v)| json!([k, canon(v)])).collect::<Vec<_>>()})
        }
    }
}
