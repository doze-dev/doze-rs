//! cty's conversion and unification rules, directly: each case converts an expression's value to a
//! type (given in cty's JSON type encoding), or unifies a list of types, and must give what Go's
//! `convert.Convert` / `convert.Unify[Unsafe]` give (`testdata/convert/go.jsonl`, from
//! `oracle-dump convert testdata/convert/cases.json`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use doze_hcl::convert::{Unified, convert, unify};
use doze_hcl::eval::{Context, eval_attribute};
use doze_hcl::function::Functions;
use doze_hcl::types::{ObjectType, Type};
use serde_json::{Value as Json, json};
use support::{canon, variables};

/// cty's JSON type encoding (`ctyjson.UnmarshalType`).
fn ty(j: &Json) -> Type {
    match j {
        Json::String(s) => match s.as_str() {
            "string" => Type::String,
            "number" => Type::Number,
            "bool" => Type::Bool,
            "dynamic" => Type::Dynamic,
            other => panic!("type {other}"),
        },
        Json::Array(a) => match a[0].as_str().unwrap() {
            "list" => Type::list(ty(&a[1])),
            "set" => Type::set(ty(&a[1])),
            "map" => Type::map(ty(&a[1])),
            "tuple" => Type::Tuple(a[1].as_array().unwrap().iter().map(ty).collect()),
            "object" => {
                let attrs = a[1]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.clone(), ty(v)))
                    .collect();
                let optional = a
                    .get(2)
                    .map(|o| {
                        o.as_array()
                            .unwrap()
                            .iter()
                            .map(|k| k.as_str().unwrap().to_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                Type::Object(ObjectType { attrs, optional })
            }
            other => panic!("type {other}"),
        },
        other => panic!("type {other}"),
    }
}

fn rust_result(case: &Json, ctx: &Context<'_>) -> Json {
    if let Some(tys) = case.get("unify") {
        let tys: Vec<Type> = tys.as_array().unwrap().iter().map(ty).collect();
        let safe = case.get("safe") == Some(&Json::Bool(true));
        return match unify(&tys, !safe) {
            Unified::None => json!({"ok": false}),
            u => {
                json!({"ok": true, "type": serde_json::from_str::<Json>(&u.ty().unwrap().to_json()).unwrap()})
            }
        };
    }
    let (v, diags) = eval_attribute(
        &format!("x = {}\n", case["expr"].as_str().unwrap()),
        "x",
        ctx,
    )
    .unwrap();
    assert!(!diags.has_errors(), "{case}: {diags:?}");
    match convert(&v, &ty(&case["type"])) {
        Ok(v) => {
            json!({"ok": true, "value": canon(&v), "type": serde_json::from_str::<Json>(&v.ty().to_json()).unwrap()})
        }
        Err(_) => json!({"ok": false}),
    }
}

#[test]
fn conversions_and_unification_match_cty() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/convert");
    let cases: Vec<Json> =
        serde_json::from_str(&std::fs::read_to_string(format!("{dir}/cases.json")).unwrap())
            .unwrap();
    let go: Vec<Json> = std::fs::read_to_string(format!("{dir}/go.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(cases.len(), go.len());
    let funcs = Functions::new();
    let ctx = Context::new(variables(), Some(&funcs));
    let mut diffs = Vec::new();
    for (case, g) in cases.iter().zip(&go) {
        let mut want = g.clone();
        let w = want.as_object_mut().unwrap();
        w.remove("id");
        w.remove("err");
        let got = rust_result(case, &ctx);
        if want != got {
            diffs.push(format!(
                "{}: {case}\n   go:   {want}\n   rust: {got}",
                case["id"]
            ));
        }
    }
    eprintln!("convert: {} cases, {} different", cases.len(), diffs.len());
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}
