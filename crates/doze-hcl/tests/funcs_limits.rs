//! What the oracle corpus cannot show: `env` reading through its lookup, and the bounds doze puts
//! where Go would allocate whatever it is asked for or recurse as deep as the input goes (catalogue
//! F19, F21).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_hcl::Value;
use doze_hcl::eval::{Context, eval_attribute};
use doze_hcl::funcs::{doze_functions, doze_functions_with_env};
use doze_hcl::function::Functions;
use std::collections::BTreeMap;
use std::sync::Arc;

fn eval(funcs: &Functions, expr: &str) -> Result<Value, String> {
    let ctx = Context::new(BTreeMap::new(), Some(funcs));
    let (v, diags) = eval_attribute(&format!("x = {expr}\n"), "x", &ctx).map_err(|d| d.detail)?;
    match diags.iter().next() {
        Some(d) if diags.has_errors() => Err(d.detail.clone()),
        _ => Ok(v),
    }
}

#[test]
fn env_reads_through_its_lookup() {
    let fs = doze_functions_with_env(Arc::new(|name: &str| {
        (name == "DOZE_TEST").then(|| "from the lookup".to_owned())
    }));
    assert_eq!(
        eval(&fs, r#"env("DOZE_TEST")"#),
        Ok(Value::string("from the lookup"))
    );
    assert_eq!(eval(&fs, r#"env("PATH", "d")"#), Ok(Value::string("d")));
    assert_eq!(eval(&fs, r#"env("PATH")"#), Ok(Value::string("")));
}

#[test]
fn env_defaults_to_the_process_environment() {
    let fs = doze_functions();
    let want = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    assert_eq!(
        eval(&fs, r#"env("CARGO_MANIFEST_DIR")"#),
        Ok(Value::string(&want))
    );
    // Names the OS cannot hold are simply not set (Rust's lookup would panic on them).
    for name in ["", "A=B", "A\\u0000B"] {
        assert_eq!(
            eval(&fs, &format!(r#"env("{name}", "unset")"#)),
            Ok(Value::string("unset"))
        );
    }
}

#[test]
fn format_widths_and_precisions_are_bounded() {
    let fs = doze_functions();
    let v = eval(&fs, r#"format("%1000000s", "x")"#).unwrap();
    assert_eq!(v.as_str().map(str::len), Some(1_000_000));
    let err = eval(&fs, r#"format("%1000001s", "x")"#).unwrap_err();
    assert!(err.contains("larger than 1000000"), "{err}");
    assert!(eval(&fs, r#"format("%.99999999999999999999f", 1)"#).is_err());
}

#[test]
fn indent_output_is_bounded() {
    let fs = doze_functions();
    assert!(eval(&fs, r#"indent(100000000, "ab")"#).is_ok());
    let err = eval(&fs, r#"indent(100000000, "a\nb")"#).unwrap_err();
    assert!(err.contains("longer than"), "{err}");
    assert!(eval(&fs, r#"indent(1000, "a\nb")"#).is_ok());
}

#[test]
fn json_nesting_is_bounded() {
    let fs = doze_functions();
    let doc = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    let ok = eval(&fs, &format!(r#"jsondecode("{}")"#, doc(512)));
    assert!(ok.is_ok(), "{ok:?}");
    let err = eval(&fs, &format!(r#"jsondecode("{}")"#, doc(513))).unwrap_err();
    assert!(err.contains("exceeded max depth"), "{err}");
    assert!(eval(&fs, &format!(r#"jsondecode("{}")"#, doc(100_000))).is_err());
}

#[test]
fn regex_nesting_is_bounded() {
    let fs = doze_functions();
    let nested = |n: usize| format!("{}a{}", "(".repeat(n), ")".repeat(n));
    let ok = eval(&fs, &format!(r#"regex("{}", "a")"#, nested(249)));
    assert!(ok.is_ok(), "{ok:?}");
    let ok = eval(
        &fs,
        &format!(r#"regex("{}", "a")"#, nested(249).replace('a', "[a]{1,2}")),
    );
    assert!(ok.is_ok(), "{ok:?}");
    let err = eval(&fs, &format!(r#"regex("{}", "a")"#, nested(250))).unwrap_err();
    assert!(err.contains("nests too deeply"), "{err}");
    let err = eval(&fs, &format!(r#"regex("{}", "a")"#, nested(100_000))).unwrap_err();
    assert!(err.contains("nests too deeply"), "{err}");
}
