//! Avro on its own: what the parser refuses, the stored text, the resolved view. (The verdicts and reasons are held to
//! Confluent's by the corpus in `tests/corpus.rs`; the stored forms also by `tests/conformance.rs`, canonical-forms.)

use super::*;
use crate::format::{ResolvedRef, SchemaType};

fn stored(text: &str) -> String {
    parse(text, &[]).unwrap().canonical(false)
}

fn refused(text: &str) -> String {
    parse(text, &[]).unwrap_err()
}

fn reference(name: &str, text: &str) -> ResolvedRef {
    ResolvedRef {
        name: name.into(),
        subject: name.into(),
        version: 1,
        kind: SchemaType::Avro,
        text: text.into(),
        direct: true,
        node: format!("{name}\x001"),
        names: Vec::new(),
    }
}

#[test]
fn what_confluents_parser_refuses_is_refused() {
    for (bad, why) in [
        (r#"{"type":"record""#, "at byte"),
        (r#""Nope""#, "Undefined name"),
        (r#"{"name":"R"}"#, "No type"),
        (r#"{"type":"record","fields":[]}"#, "No name"),
        (r#"{"type":"record","name":"R"}"#, "no fields"),
        (
            r#"{"type":"record","name":"1R","fields":[]}"#,
            "Illegal character",
        ),
        (
            r#"{"type":"record","name":"R","namespace":"a..b","fields":[]}"#,
            "Illegal character",
        ),
        (
            r#"{"type":"record","name":"int","fields":[]}"#,
            "named after primitives",
        ),
        (
            r#"{"type":"record","name":"R","fields":[{"name":"a","type":"int"},{"name":"a","type":"long"}]}"#,
            "Duplicate field",
        ),
        (
            r#"{"type":"record","name":"R","fields":[{"name":"a"}]}"#,
            "No field type",
        ),
        (
            r#"{"type":"record","name":"R","fields":[{"name":"a","type":"double","default":""}]}"#,
            "empty String",
        ),
        (
            r#"{"type":"record","name":"R","fields":[{"name":"a","type":"int","order":"up"}]}"#,
            "Illegal order",
        ),
        (
            r#"{"type":"enum","name":"E","symbols":["A","A"]}"#,
            "Duplicate enum symbol",
        ),
        (
            r#"{"type":"enum","name":"E","symbols":["A"],"default":"B"}"#,
            "not in the enum symbol set",
        ),
        (r#"{"type":"enum","name":"E"}"#, "no symbols"),
        (r#"{"type":"fixed","name":"F","size":-1}"#, "size"),
        (r#"["int","int"]"#, "Duplicate in union"),
        (r#"["int",["null"]]"#, "Nested union"),
        (r#"{"type":"array"}"#, "no items"),
        (r#"{"type":"map"}"#, "no values"),
        (
            r#"{"type":"record","name":"S","fields":[{"name":"a","type":{"type":"fixed","name":"F","size":4}},{"name":"b","type":{"type":"fixed","name":"F","size":4}}]}"#,
            "redefine",
        ),
        (
            r#"{"type":"record","name":"R","aliases":"X","fields":[]}"#,
            "aliases",
        ),
        ("5", "No type"),
    ] {
        let e = refused(bad);
        assert!(e.contains(why), "{bad}: {e}");
    }
}

#[test]
fn what_confluent_accepts_is_accepted() {
    for ok in [
        r#"{"type":"record","name":"R","fields":[{"name":"a","type":"string","default":5}]}"#,
        r#"{"type":"record","name":"R","fields":[{"name":"a","type":["null","string"],"default":"x"}]}"#,
        r#"{"type":"record","name":"Tree","fields":[{"name":"kids","type":{"type":"array","items":"Tree"}}]}"#,
        r#"{"type":"record","name":"R","namespace":"","fields":[]}"#,
        r#"{"type":"record","name":"Ünïcode","fields":[{"name":"_x1","type":"int"}]}"#,
        r#"{"type":"bytes","logicalType":"decimal","precision":"not a number"}"#,
        r#"{"type":{"type":"array","items":"int"}}"#,
        r#"["null",{"type":"record","name":"A","fields":[]},{"type":"record","name":"B","fields":[]}]"#,
        r#"{"type":"enum","name":"E","symbols":[]}"#,
    ] {
        assert!(parse(ok, &[]).is_ok(), "{ok}: {:?}", parse(ok, &[]).err());
    }
}

#[test]
fn the_stored_text_is_avros_own_order_and_namespaces_are_written_once() {
    assert_eq!(
        stored(r#"{"type":"string","doc":"d","aliases":["x"]}"#),
        r#""string""#
    );
    assert_eq!(
        stored(r#"{"logicalType":"timestamp-millis","type":"long"}"#),
        r#"{"type":"long","logicalType":"timestamp-millis"}"#
    );
    assert_eq!(
        stored(
            r#"{"fields":[{"type":"int","name":"a","default":1,"x":true}],"namespace":"n","name":"R","type":"record","doc":"d"}"#
        ),
        r#"{"type":"record","name":"R","namespace":"n","doc":"d","fields":[{"name":"a","type":"int","default":1,"x":true}]}"#
    );
    // A nested type in the same namespace has none written; a reference is as short as the namespace allows.
    assert_eq!(
        stored(
            r#"{"type":"record","name":"a.b.R","fields":[{"name":"i","type":{"type":"record","name":"a.b.I","fields":[]}},{"name":"j","type":"a.b.I"},{"name":"k","type":{"type":"enum","name":"c.E","symbols":["X"]}}]}"#
        ),
        r#"{"type":"record","name":"R","namespace":"a.b","fields":[{"name":"i","type":{"type":"record","name":"I","fields":[]}},{"name":"j","type":"I"},{"name":"k","type":{"type":"enum","name":"E","namespace":"c","symbols":["X"]}}]}"#
    );
    // A type in no namespace inside one that has a namespace says so.
    assert_eq!(
        stored(
            r#"{"type":"record","name":"R","namespace":"n","fields":[{"name":"i","type":{"type":"fixed","name":"F","namespace":"","size":2}}]}"#
        ),
        r#"{"type":"record","name":"R","namespace":"n","fields":[{"name":"i","type":{"type":"fixed","name":"F","namespace":"","size":2}}]}"#
    );
    // Jackson's escapes, not Go's: '<' is itself, a control character is \u00XX.
    assert_eq!(
        stored("{\"type\":\"record\",\"name\":\"R\",\"doc\":\"<\\u0001>\",\"fields\":[]}"),
        "{\"type\":\"record\",\"name\":\"R\",\"doc\":\"<\\u0001>\",\"fields\":[]}"
    );
}

#[test]
fn normalizing_sorts_the_other_attributes() {
    let s = parse(r#"{"type":"long","z":1,"logicalType":"x","a":2}"#, &[]).unwrap();
    assert_eq!(
        s.canonical(true),
        r#"{"type":"long","a":2,"logicalType":"x","z":1}"#
    );
    assert_eq!(
        s.canonical(false),
        r#"{"type":"long","z":1,"logicalType":"x","a":2}"#
    );
}

#[test]
fn a_referenced_type_is_known_by_its_name_and_written_short() {
    let geo = reference(
        "com.acme.Geo",
        r#"{"type":"record","name":"Geo","namespace":"com.acme","fields":[]}"#,
    );
    let main = r#"{"type":"record","name":"P","namespace":"com.acme","fields":[{"name":"g","type":"com.acme.Geo"},{"name":"h","type":"Geo"}]}"#;
    let s = parse(main, &[geo]).unwrap();
    assert_eq!(
        s.canonical(false),
        r#"{"type":"record","name":"P","namespace":"com.acme","fields":[{"name":"g","type":"Geo"},{"name":"h","type":"Geo"}]}"#
    );
    assert!(parse(main, &[]).unwrap_err().contains("Undefined name"));
}

#[test]
fn the_resolved_view_writes_each_referenced_type_out_once() {
    let lib = r#"{"type":"record","name":"Lib","namespace":"l","fields":[
        {"name":"c","type":{"type":"enum","name":"Colour","symbols":["RED"]}},
        {"name":"h","type":{"type":"fixed","name":"Hash","size":4}},
        {"name":"xs","type":{"type":"array","items":{"type":"record","name":"Item","fields":[{"name":"n","type":"int"}]}}},
        {"name":"m","type":{"type":"map","values":{"type":"record","name":"Val","fields":[{"name":"v","type":"long"}]}}},
        {"name":"u","type":["null",{"type":"record","name":"InUnion","fields":[]}]}]}"#;
    let main = r#"{"type":"record","name":"Main","fields":[{"name":"a","type":"l.Colour"},{"name":"b","type":"l.Hash"},{"name":"c","type":{"type":"array","items":"l.Item"}},{"name":"d","type":{"type":"map","values":"l.Val"}},{"name":"e","type":"l.InUnion"},{"name":"f","type":"l.Colour"}]}"#;
    let got = inline(main, &[lib]).unwrap();
    for want in [
        r#""name":"l.Colour""#,
        r#""name":"l.Hash""#,
        r#""name":"l.Item""#,
        r#""name":"l.Val""#,
        r#""name":"l.InUnion""#,
        r#"{"name":"f","type":"l.Colour"}"#,
    ] {
        assert!(got.contains(want), "{want} not in {got}");
    }
    assert!(
        parse(&got, &[]).is_ok(),
        "the inlined schema does not parse alone: {got}"
    );
    assert!(inline(main, &["{"]).is_err());
}

#[test]
fn a_recursive_type_compares_with_itself_and_stops() {
    let tree = r#"{"type":"record","name":"Tree","fields":[{"name":"v","type":"int"},{"name":"kids","type":{"type":"array","items":"Tree"}}]}"#;
    let (a, b) = (parse(tree, &[]).unwrap(), parse(tree, &[]).unwrap());
    assert!(
        a.can_read(&b, crate::format::Role::New, crate::format::Role::Old)
            .is_empty()
    );
}
