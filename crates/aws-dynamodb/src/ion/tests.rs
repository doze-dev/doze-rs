#![allow(clippy::unwrap_used)]

use aws_expr::Item;

use super::*;

fn n(s: &str) -> Value {
    Value::N(Number::parse(s).unwrap())
}

#[test]
fn what_an_export_writes_is_read_back() {
    let mut item = Item::new();
    item.insert("Id".into(), n("103"));
    item.insert("Price".into(), n("2000.5"));
    item.insert("Big".into(), n("1E+40"));
    item.insert(
        "Authors".into(),
        Value::Ss(["A".into(), "B\"q".into()].into()),
    );
    item.insert("Ns".into(), Value::Ns([Number::from_i64(1)].into()));
    item.insert("Bin".into(), Value::B(vec![0, 1, 2]));
    item.insert("Bs".into(), Value::Bs([vec![9]].into()));
    item.insert(
        "odd name".into(),
        Value::L(vec![Value::Null, Value::Bool(true)]),
    );
    item.insert(
        "M".into(),
        Value::M([("x\ny".into(), Value::S(String::new()))].into()),
    );
    let text = line("Item", &Value::M(item.clone()));
    assert!(text.starts_with("$ion_1_0 {Item:{"));
    let mut read = read_all(&text).unwrap();
    let Some(Ion::Struct(mut top)) = read.pop() else {
        panic!("{text}")
    };
    let Some((_, inner)) = top.pop() else {
        panic!()
    };
    assert_eq!(to_value(inner).unwrap(), Value::M(item));
}

#[test]
fn the_guides_example_reads() {
    let text = "$ion_1_0 {\n Item:{\n Authors:$dynamodb_SS::[\"Author1\",\"Author2\"],\n Id:103.,\n InPublication:false,\n PageCount:6d2,\n Price:2d3,\n Title:'''Book ''' '''103''' // a comment\n }\n}\n";
    let read = read_all(text).unwrap();
    let [Ion::Struct(top)] = read.as_slice() else {
        panic!("{read:?}")
    };
    let Some((name, Ion::Struct(fields))) = top.first() else {
        panic!()
    };
    assert_eq!(name, "Item");
    let get = |k: &str| fields.iter().find(|(f, _)| f == k).map(|(_, v)| v).unwrap();
    assert_eq!(get("PageCount"), &Ion::Num("6E2".into()));
    assert_eq!(get("Id"), &Ion::Num("103".into()));
    assert_eq!(get("Title"), &Ion::Str("Book 103".into()));
    assert!(matches!(get("Authors"), Ion::List(_, Some(a)) if a == "$dynamodb_SS"));
}

#[test]
fn what_has_no_dynamodb_form_is_refused() {
    for bad in [
        "{a:2020-01-01T}",
        "{a:(x y)}",
        "{a:{{\"clob\"}}}",
        "{a:",
        "{a:'x}",
        "{a 1}",
    ] {
        assert!(read_all(bad).is_err(), "{bad}");
    }
    assert!(
        to_value(Ion::List(
            vec![Ion::Num("1".into())],
            Some("$dynamodb_SS".into())
        ))
        .is_err()
    );
}
