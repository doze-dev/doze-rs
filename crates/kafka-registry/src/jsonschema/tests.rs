//! JSON Schema on its own: what is refused, the stored text, references, and comparisons the corpus does not reach.
//! (Verdicts and reasons are held to Confluent's by `tests/corpus.rs`; registrations by `tests/conformance.rs`.)

mod compare;

use super::number::Dec;
use super::*;
use crate::format::{ResolvedRef, Role, SchemaType};

fn read(text: &str) -> ParsedJsonSchema {
    parse(text, &[]).unwrap_or_else(|e| panic!("{text}: {e}"))
}

fn refused(text: &str) -> String {
    parse(text, &[]).map(|_| ()).expect_err(text)
}

/// The reasons a reader of `new` gives for data written with `old` (BACKWARD).
fn reasons(old: &str, new: &str) -> Vec<String> {
    read(new)
        .can_read(&read(old), Role::New, Role::Old)
        .into_iter()
        .map(|r| r.kind)
        .collect()
}

fn reference(
    name: &str,
    node: &str,
    text: &str,
    direct: bool,
    names: &[(&str, &str)],
) -> ResolvedRef {
    ResolvedRef {
        name: name.into(),
        subject: node.into(),
        version: 1,
        kind: SchemaType::Json,
        text: text.into(),
        direct,
        node: format!("{node}\x001"),
        names: names
            .iter()
            .map(|(n, s)| ((*n).to_owned(), format!("{s}\x001")))
            .collect(),
    }
}

#[test]
fn what_is_not_a_schema_of_its_draft_is_refused() {
    for (bad, why) in [
        ("[1,2", "Invalid JSON"),
        (r#"{"type":"nonsense"}"#, "is not a type"),
        (r#"{"type":["string","string"]}"#, "named twice"),
        (r#"{"type":[]}"#, "must not be empty"),
        ("42", "must be an object or a boolean"),
        (
            r#"{"properties":{"a":7}}"#,
            "#/properties/a: a schema must be",
        ),
        (r#"{"properties":[]}"#, "must be an object"),
        (r#"{"minLength":-1}"#, "non-negative integer"),
        (r#"{"maxItems":1.5}"#, "non-negative integer"),
        (r#"{"multipleOf":0}"#, "greater than 0"),
        (r#"{"required":["a","a"]}"#, "named twice"),
        (r#"{"required":[1]}"#, "array of strings"),
        (r#"{"enum":[]}"#, "must not be empty"),
        (r#"{"enum":[1,1.0]}"#, "distinct"),
        (r#"{"allOf":[]}"#, "must not be empty"),
        (r#"{"pattern":"(a"}"#, "unclosed '('"),
        (r#"{"pattern":"*a"}"#, "nothing to repeat"),
        (r#"{"patternProperties":{"[a":{}}}"#, "unclosed '['"),
        (
            r#"{"$schema":"http://example.com/schema"}"#,
            "is not a JSON Schema draft",
        ),
        (
            r##"{"$ref":"#/definitions/missing"}"##,
            "unresolved reference",
        ),
        (r#"{"$ref":"other.json"}"#, "unresolved reference"),
        (
            r##"{"properties":{"a":{"$ref":"#nowhere"}}}"##,
            "unresolved reference",
        ),
        // Draft 4 has no boolean schemas and boolean exclusive limits that need their limit.
        (
            r#"{"$schema":"http://json-schema.org/draft-04/schema#","properties":{"a":true}}"#,
            "must be an object",
        ),
        (
            r#"{"$schema":"http://json-schema.org/draft-04/schema#","exclusiveMinimum":true}"#,
            "needs minimum",
        ),
        (
            r#"{"$schema":"http://json-schema.org/draft-04/schema#","exclusiveMinimum":3,"minimum":1}"#,
            "boolean",
        ),
        // 2020-12 has prefixItems for positions; items is one schema.
        (
            r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","items":[{}]}"#,
            "a schema must be",
        ),
    ] {
        let got = refused(bad);
        assert!(got.contains(why), "{bad}: {got}, want {why:?}");
    }
}

#[test]
fn what_its_draft_allows_is_accepted() {
    for good in [
        "true",
        "false",
        "{}",
        r#"{"type":["string","null"],"maxLength":3}"#,
        r#"{"$schema":"http://json-schema.org/draft-04/schema#","minimum":1,"exclusiveMinimum":true,"additionalProperties":false}"#,
        r#"{"$schema":"http://json-schema.org/draft-06/schema","const":1,"contains":{"type":"string"}}"#,
        r##"{"$schema":"https://json-schema.org/draft/2019-09/schema","$defs":{"a":{"type":"string"}},"$ref":"#/$defs/a","minLength":1}"##,
        r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","prefixItems":[{"type":"string"}],"items":false}"#,
        r#"{"$schema":"http://json-schema.org/schema","dependentRequired":{"a":["b"]}}"#,
        // A pointer into an array, an escaped name, an anchor, a reference in an unused definition.
        r##"{"allOf":[{"type":"string"},{"$ref":"#/allOf/0"}]}"##,
        r##"{"definitions":{"a/b c":{"type":"string"}},"properties":{"x":{"$ref":"#/definitions/a~1b%20c"}}}"##,
        r##"{"definitions":{"a":{"$id":"#thing","type":"string"}},"properties":{"x":{"$ref":"#thing"}}}"##,
        r##"{"definitions":{"unused":{"$ref":"#/nowhere"}}}"##,
        // Keywords a draft does not have are annotations; Java's regular expressions are not Go's.
        r#"{"x-vendor":{"type":7},"pattern":"^(?=a)[\\w-]+$"}"#,
        r#"{"enum":[]}"#
            .replace(
                "{",
                r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","#,
            )
            .as_str(),
    ] {
        read(good);
    }
}

#[test]
fn the_stored_text_is_compact_and_a_repeated_key_is_kept_once() {
    let s =
        read("{ \"type\" : \"object\",\n  \"properties\" : { \"a\" : { \"type\":\"string\" } } }");
    assert_eq!(
        s.canonical(false),
        r#"{"type":"object","properties":{"a":{"type":"string"}}}"#
    );
    assert_eq!(s.canonical(true), s.canonical(false));
    let s = read(r#"{"type":"string","title":"a<b\u0001","type":"integer","n":1.50}"#);
    assert_eq!(
        s.canonical(false),
        "{\"type\":\"integer\",\"title\":\"a<b\\u0001\",\"n\":1.50}"
    );
}

#[test]
fn a_name_means_what_the_schema_using_it_says() {
    // "x.json" is the leaf in mid, and mid in top (recorded: nested-references).
    let leaf = r#"{"type":"object","properties":{"v":{"type":"string"}}}"#;
    let mid = r#"{"type":"object","properties":{"p0":{"$ref":"x.json"}}}"#;
    let refs = [
        reference("x.json", "leaf", leaf, false, &[]),
        reference("x.json", "mid", mid, true, &[("x.json", "leaf")]),
    ];
    let top = parse(
        r#"{"type":"object","properties":{"p0":{"$ref":"x.json"}}}"#,
        &refs,
    )
    .unwrap();
    let changed_leaf = r#"{"type":"object","properties":{"v":{"type":"integer"}}}"#;
    let refs2 = [
        reference("x.json", "leaf2", changed_leaf, false, &[]),
        reference("x.json", "mid", mid, true, &[("x.json", "leaf2")]),
    ];
    let top2 = parse(
        r#"{"type":"object","properties":{"p0":{"$ref":"x.json"}}}"#,
        &refs2,
    )
    .unwrap();
    let why: Vec<_> = top2
        .can_read(&top, Role::New, Role::Old)
        .into_iter()
        .map(|r| r.path)
        .collect();
    assert_eq!(why, ["#/properties/p0/properties/p0/properties/v"]);
    // A fragment of a referenced schema, and a reference inside it that is its own.
    let defs = r##"{"definitions":{"a":{"$ref":"#/definitions/b"},"b":{"type":"string"}}}"##;
    let refs = [reference("d.json", "defs", defs, true, &[])];
    let s = parse(
        r#"{"properties":{"x":{"$ref":"d.json#/definitions/a"}}}"#,
        &refs,
    )
    .unwrap();
    let plain = read(r#"{"properties":{"x":{"type":"string"}}}"#);
    assert!(s.can_read(&plain, Role::New, Role::Old).is_empty());
    // A name the schema does not declare reaches nothing.
    assert!(
        parse(r#"{"$ref":"e.json"}"#, &refs)
            .unwrap_err()
            .contains("unresolved")
    );
}

#[test]
fn an_address_changed_to_a_number_is_breaking_through_a_reference() {
    let address =
        r#"{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}"#;
    let refs = [reference("address.json", "address", address, true, &[])];
    let person = parse(
        r#"{"type":"object","properties":{"home":{"$ref":"address.json"}}}"#,
        &refs,
    )
    .unwrap();
    let changed = read(r#"{"type":"object","properties":{"home":{"type":"number"}}}"#);
    let why = changed.can_read(&person, Role::New, Role::Old);
    assert_eq!(why.len(), 1);
    assert_eq!(why[0].kind, "TYPE_CHANGED");
    assert_eq!(why[0].path, "#/properties/home");
}

#[test]
fn additional_items_and_dependencies_are_named_as_confluent_names_them() {
    let tuple = |extra: &str| {
        format!(r#"{{"type":"array","items":[{{"type":"string"}}],"additionalItems":{extra}}}"#)
    };
    for (old, new, want) in [
        (tuple("false"), tuple(r#"{"type":"string"}"#), None),
        (
            tuple(r#"{"type":"string"}"#),
            tuple("false"),
            Some("ADDITIONAL_ITEMS_REMOVED"),
        ),
        (
            tuple(r#"{"type":"integer"}"#),
            tuple(r#"{"type":"number"}"#),
            None,
        ),
        (
            tuple(r#"{"type":"number"}"#),
            tuple(r#"{"type":"integer"}"#),
            Some("ADDITIONAL_ITEMS_NARROWED"),
        ),
        (
            r#"{"dependencies":{"a":["b"]}}"#.into(),
            r#"{"dependencies":{"a":["b","c"]}}"#.into(),
            Some("DEPENDENCY_ARRAY_EXTENDED"),
        ),
        (
            r#"{"dependencies":{"a":["b","c"]}}"#.into(),
            r#"{"dependencies":{"a":["b"]}}"#.into(),
            None,
        ),
        (
            r#"{"dependencies":{"a":["b"]}}"#.into(),
            r#"{"dependencies":{"a":["c"]}}"#.into(),
            Some("DEPENDENCY_ARRAY_EXTENDED"),
        ),
        (
            r#"{"dependencies":{}}"#.into(),
            r#"{"dependencies":{"a":["b"]}}"#.into(),
            Some("DEPENDENCY_ARRAY_ADDED"),
        ),
        (
            r#"{"dependencies":{"a":["b"]}}"#.into(),
            r#"{"dependencies":{}}"#.into(),
            None,
        ),
        (
            r#"{"dependencies":{"a":["b","c"]}}"#.into(),
            r#"{"dependencies":{"a":["c","b"]}}"#.into(),
            None,
        ),
    ] {
        assert_eq!(
            reasons(&old, &new),
            want.into_iter().collect::<Vec<_>>(),
            "{old} -> {new}"
        );
    }
}

#[test]
fn numbers_compare_by_value_and_multiples_are_exact() {
    let d = |s: &str| Dec::parse(s).unwrap();
    assert_eq!(d("1"), d("1.0"));
    assert_eq!(d("1e2"), d("100"));
    assert_eq!(d("-0"), d("0.0"));
    assert!(
        d("-2") < d("-1.5") && d("-1.5") < d("0") && d("0") < d("1e-300") && d("1e-300") < d("9")
    );
    assert!(d("10") > d("9.99999999999999999999999"));
    assert!(d("0.3").is_multiple_of(&d("0.1")));
    assert!(d("12").is_multiple_of(&d("4")));
    assert!(!d("10").is_multiple_of(&d("4")));
    assert!(!d("0.15").is_multiple_of(&d("0.1")));
    assert!(d("7e30").is_multiple_of(&d("7e2")) && !d("1e30").is_multiple_of(&d("7")));
    assert!(d("123456789012345678901234567890123456789").is_multiple_of(&d("3")));
    assert!(!d("5").is_multiple_of(&d("0")));
    assert!(Dec::parse("1.").is_none() && Dec::parse("x").is_none());
    // 0.3 is a multiple of 0.1: widening multipleOf from 0.3 to 0.1 accepts more.
    let num = |m: &str| format!(r#"{{"type":"number","multipleOf":{m}}}"#);
    assert!(reasons(&num("0.3"), &num("0.1")).is_empty());
    assert_eq!(reasons(&num("0.1"), &num("0.3")), ["MULTIPLE_OF_EXPANDED"]);
}
