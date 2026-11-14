//! Protobuf on its own: what the parser and the linker refuse, the stored text, descriptors, references, message
//! indexes. (Verdicts and reasons are held to Confluent's by the corpus in `tests/corpus.rs`; the stored forms also by
//! `tests/conformance.rs`, protobuf-canonical-forms and protobuf-descriptor-with-import.)

use super::*;
use crate::format::{ResolvedRef, SchemaType};
use crate::registry::{Origin, RegisterResult, Registry, SchemaInput};
use crate::store::{MemoryStorage, Reference};

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
        kind: SchemaType::Protobuf,
        text: text.into(),
        direct: true,
        node: format!("{name}\x001"),
        names: Vec::new(),
    }
}

/// Texts and the words their refusal must contain.
const REFUSED: &[(&str, &str)] = &[
    (
        "syntax = \"proto3\"; message {",
        "Syntax error in :1:28: expected a name",
    ),
    ("syntax = \"proto4\";", "proto2\" or \"proto3"),
    ("edition = \"2023\";", "editions are not supported"),
    (
        "message M { int32 a = 1; }",
        "a label (optional, required or repeated) is required in proto2",
    ),
    (
        "syntax = \"proto3\"; message M { required int32 a = 1; }",
        "required fields are not allowed in proto3",
    ),
    (
        "syntax = \"proto3\"; message M { int32 a = 0; }",
        "out of range",
    ),
    (
        "syntax = \"proto3\"; message M { int32 a = 536870912; }",
        "out of range",
    ),
    (
        "syntax = \"proto3\"; message M { int32 a = 19000; }",
        "reserved for the implementation",
    ),
    (
        "syntax = \"proto3\"; message M { int32 a = 1; string b = 1; }",
        "already used by a",
    ),
    (
        "syntax = \"proto3\"; message M { int32 a = 1; string a = 2; }",
        "already defined",
    ),
    (
        "syntax = \"proto3\"; message M { reserved 2; int32 a = 2; }",
        "reserved number 2",
    ),
    (
        "syntax = \"proto3\"; message M { reserved \"a\"; int32 a = 2; }",
        "reserved name",
    ),
    (
        "syntax = \"proto3\"; message M { N n = 1; }",
        "unknown type N",
    ),
    (
        "syntax = \"proto3\"; message M { map<float, int32> m = 1; }",
        "map key type float",
    ),
    (
        "syntax = \"proto3\"; message M { oneof o { repeated int32 a = 1; } }",
        "must not have labels",
    ),
    (
        "syntax = \"proto3\"; message M { oneof o { } }",
        "has no fields",
    ),
    (
        "syntax = \"proto3\"; enum E { A = 1; }",
        "first value of a proto3 enum must be zero",
    ),
    ("syntax = \"proto3\"; enum E { }", "at least one value"),
    (
        "syntax = \"proto3\"; enum E { A = 0; B = 0; }",
        "reuses number 0",
    ),
    (
        "syntax = \"proto3\"; enum E { option allow_alias = true; A = 0; B = 1; }",
        "no two values share",
    ),
    (
        "syntax = \"proto3\"; enum E { A = 0; } enum F { A = 0; }",
        "\"A\" is already defined",
    ),
    (
        "syntax = \"proto3\"; message M { int32 a = 1 [default = 1]; }",
        "not allowed in proto3",
    ),
    (
        "syntax = \"proto3\"; message M { int32 a_b = 1; int32 aB = 2; }",
        "JSON name",
    ),
    (
        "syntax = \"proto3\"; message M { extensions 100 to 199; }",
        "not allowed in proto3",
    ),
    (
        "syntax = \"proto3\"; message M { int32 a = 1 [packed = true]; }",
        "packed",
    ),
    (
        "syntax = \"proto3\"; option java_package = 1;",
        "wrong type",
    ),
    (
        "syntax = \"proto3\"; option no_such = true;",
        "has no such option",
    ),
    (
        "syntax = \"proto3\"; option (no.such) = true;",
        "unknown extension",
    ),
    (
        "syntax = \"proto3\"; import \"other.proto\";",
        "import \"other.proto\" was not found",
    ),
    (
        "message M { optional int32 a = 1 [default = \"x\"]; }",
        "default value does not fit",
    ),
    ("message M { optional group g = 1 { } }", "capital letter"),
    (
        "syntax = \"proto3\"; service S { rpc A (B) returns (B); }",
        "unknown type B",
    ),
    (
        "syntax = \"proto3\"; message M { string s = 1 /* never closed",
        "unterminated block comment",
    ),
    (
        "syntax = \"proto3\"; message M { string s = 1 [default = \"x\n\"]; }",
        "unterminated string",
    ),
    ("not a schema", "Syntax error"),
];

#[test]
fn what_the_language_refuses_is_refused() {
    for &(bad, why) in REFUSED {
        let got = refused(bad);
        assert!(
            got.contains(why),
            "{bad}\n  refused with: {got}\n  expected: {why}"
        );
    }
}

#[test]
fn what_the_language_allows_is_read_and_stored() {
    let text = "// lead\nsyntax = 'proto2';\npackage a.b;\nimport \"google/protobuf/descriptor.proto\";\n\
        extend google.protobuf.FieldOptions { optional string tag = 50000; }\n\
        message M {\n  extensions 100 to max;\n  reserved 5 to 7, 9;\n  reserved \"x\";\n\
        optional string s = 1 [(tag) = \"t\", default = \"a\" \"b\"];\n  optional E e = 2 [default = B];\n\
        required int64 n = 0x3;\n  optional double d = 4 [default = -inf];\n  repeated group G = 10 { optional int32 z = 1; }\n\
        oneof k { int32 i = 11; group H = 12 { optional int32 y = 1; } }\n  map<string, M> m = 13;\n\
        enum E { A = 0; B = -1; reserved 2 to max; }\n  extend M { optional int32 ext = 100; }\n}\n\
        service S { option deprecated = true; rpc R (M) returns (stream .a.b.M) { option deprecated = true; } }";
    let s = stored(text);
    assert_eq!(
        s,
        r#"syntax = 'proto2';
package a.b;

import "google/protobuf/descriptor.proto";

message M {
  reserved 5 to 7, 9;
  reserved "x";

  optional string s = 1 [
    (tag) = "t",
    default = "a" "b"
  ];
  optional E e = 2 [default = B];
  required int64 n = 0x3;
  optional double d = 4 [default = -inf];
  map<string, M> m = 13;

  oneof k {
    int32 i = 11;
    group H = 12 {
      optional int32 y = 1;
    }
  }

  repeated group G = 10 {
    optional int32 z = 1;
  }

  enum E {
    reserved 2 to max;
    A = 0;
    B = -1;
  }

  extend M {
    optional int32 ext = 100;
  }

  extensions 100 to max;
}

extend google.protobuf.FieldOptions {
  optional string tag = 50000;
}

service S {
  option deprecated = true;
  rpc R (M) returns (stream .a.b.M) {
    option deprecated = true;
  };
}
"#
    );
    // The stored text reads back as itself.
    assert_eq!(stored(&s), s);
}

/// Printing is stable: what is stored reads back to the same text, for every schema of the corpus.
#[test]
fn the_stored_text_of_every_corpus_schema_reads_back_as_itself() {
    let raw = include_str!("../../tests/data/compat/protobuf.json");
    let corpus: serde_json::Value = serde_json::from_str(raw).unwrap();
    let mut read = 0;
    for c in corpus["cases"].as_array().unwrap() {
        for side in ["old", "new"] {
            if let Ok(s) = parse(c[side].as_str().unwrap(), &[]) {
                let text = s.canonical(false);
                assert_eq!(stored(&text), text, "{}", c["name"]);
                read += 1;
            }
        }
    }
    assert!(read > 900, "{read}");
}

/// Descriptors registered with Confluent 7.7.1 and the text it stored (`tests/data/protobuf/protodesc.json`).
#[test]
fn a_descriptor_is_stored_as_confluent_stores_it() {
    let raw = include_str!("../../tests/data/protobuf/protodesc.json");
    let cases: serde_json::Value = serde_json::from_str(raw).unwrap();
    let cases = cases.as_array().unwrap();
    assert!(cases.len() >= 5);
    for c in cases {
        let got = parse(c["descriptor"].as_str().unwrap(), &[]);
        if c["error"].as_str().is_some_and(|e| !e.is_empty()) {
            assert!(got.is_err(), "{}", c["name"]);
            continue;
        }
        let got = got.unwrap_or_else(|e| panic!("{}: {e}", c["name"]));
        assert_eq!(
            got.canonical(false),
            c["canonical"].as_str().unwrap(),
            "{}",
            c["name"]
        );
    }
}

#[test]
fn garbage_is_not_a_descriptor() {
    for bad in ["not a schema", "bm90IGEgZGVzY3JpcHRvcg==", "AAAA", "===="] {
        assert!(parse(bad, &[]).is_err(), "{bad:?}");
    }
    // A descriptor whose import is missing is refused as Confluent words it.
    let main = "CgptYWluLnByb3RvEgNhcHAaCWRlcC5wcm90byInCgZQZXJzb24SHQoEaG9tZRgBIAEoCzIJLmRlcC5BZGRyUgRob21lYgZwcm90bzM=";
    assert_eq!(refused(main), "Could not resolve type: .dep.Addr");
    let dep = reference(
        "dep.proto",
        "syntax = \"proto3\"; package dep; message Addr { string city = 1; }",
    );
    assert!(parse(main, &[dep]).is_ok());
    // Found by fuzzing: a descriptor importing a file no reference gives, though none of its types is used, is
    // refused (its stored text would name an import that cannot be read again).
    let unused = "CgptlWuYLnByb3RvEgNhcHAaCWRlcC5wcm90byInCgZQZXJzb24SHUoEaG9tZRgBIAEoCzIJLmRlcC5BZGRyUgRob21lYgZwcm90bzM=";
    assert_eq!(refused(unused), "Could not resolve import: dep.proto");
}

#[test]
fn imports_are_found_by_reference_name_and_public_imports_pass_through() {
    let base = reference(
        "base.proto",
        "syntax = \"proto3\"; package base; message B { int32 x = 1; }",
    );
    let mid = reference(
        "mid.proto",
        "syntax = \"proto3\"; import public \"base.proto\"; package mid; message C {}",
    );
    let hidden = reference(
        "hidden.proto",
        "syntax = \"proto3\"; import \"base.proto\"; package hid; message D {}",
    );
    let through =
        "syntax = \"proto3\"; import \"mid.proto\"; message M { base.B b = 1; mid.C c = 2; }";
    assert!(parse(through, &[base.clone(), mid]).is_ok());
    // A type of a file imported by an import (not publicly) is not seen.
    let not_seen = "syntax = \"proto3\"; import \"hidden.proto\"; message M { base.B b = 1; }";
    let e = parse(not_seen, &[base, hidden]).unwrap_err();
    assert!(e.contains("unknown type base.B"), "{e}");
    let wkt = "syntax = \"proto3\"; import \"google/protobuf/struct.proto\"; import \"google/protobuf/api.proto\";\
        message M { google.protobuf.Struct s = 1; google.protobuf.NullValue n = 2; google.protobuf.Api a = 3; }";
    assert!(parse(wkt, &[]).is_ok());
}

#[test]
fn names_resolve_from_the_innermost_scope() {
    let text = "syntax = \"proto3\"; package p; message A { message B {} } \
        message C { message A {} A.B b = 1; }";
    // Inside C, `A` is C.A, which has no B: protoc refuses rather than looking outward.
    assert!(refused(text).contains("unknown type A.B"));
    let ok = "syntax = \"proto3\"; package p; message A { message B {} } message C { .p.A.B b = 1; p.A.B c = 2; A.B d = 3; }";
    assert!(parse(ok, &[]).is_ok());
}

#[test]
fn message_indexes_name_messages_as_confluents_serializers_number_them() {
    let s = parse(
        "syntax = \"proto3\"; package p; message A { message X {} message Y { message Z {} } } message B {}",
        &[],
    )
    .unwrap();
    assert_eq!(s.message_at(&[0]), Some("p.A"));
    assert_eq!(s.message_at(&[1]), Some("p.B"));
    assert_eq!(s.message_at(&[0, 1, 0]), Some("p.A.Y.Z"));
    assert_eq!(s.message_at(&[2]), None);
    assert_eq!(s.message_at(&[]), None);
}

#[test]
fn deep_nesting_is_refused_not_overflowed() {
    let deep = format!(
        "syntax = \"proto3\"; {}{}",
        "message M { ".repeat(5000),
        "}".repeat(5000)
    );
    assert!(refused(&deep).contains("nested too deeply"));
    let value = format!(
        "syntax = \"proto3\"; option (x) = {}{};",
        "{ a ".repeat(5000),
        "}".repeat(5000)
    );
    assert!(refused(&value).contains("nested too deeply"));
}

fn register(r: &Registry, subject: &str, text: &str, refs: Vec<Reference>) -> Result<u32, String> {
    let input = SchemaInput {
        schema: Some(text.into()),
        schema_type: Some("PROTOBUF".into()),
        references: Some(refs),
    };
    match r.register(subject, &input, false, &Origin::default()) {
        Ok(RegisterResult::Id(id)) => Ok(id),
        Ok(other) => Err(format!("{other:?}")),
        Err(e) => Err(e.message),
    }
}

/// Ported from the Go edition's `TestReferencesInJSONAndProtobuf` (its Protobuf half): a schema that imports another
/// through a reference registers, and a check against it compares the messages.
#[test]
fn a_schema_with_an_import_registers_and_is_compared() {
    let r = Registry::open(Box::new(MemoryStorage::default())).unwrap();
    register(
        &r,
        "proto-dep",
        "syntax = \"proto3\"; package dep; message Addr { string city = 1; }",
        vec![],
    )
    .unwrap();
    let dep = Reference {
        name: "dep.proto".into(),
        subject: "proto-dep".into(),
        version: 1,
    };
    let main = "syntax = \"proto3\"; import \"dep.proto\"; message P { dep.Addr home = 1; }";
    register(&r, "proto-main", main, vec![dep.clone()]).unwrap();
    let retyped = SchemaInput::of(
        SchemaType::Protobuf,
        "syntax = \"proto3\"; message P { string home = 1; }",
    );
    let (_, found) = r
        .check("proto-main", Some("latest"), &retyped, false, None)
        .unwrap();
    assert!(!found.is_empty());
    // Without its reference the import is not found.
    let e = register(&r, "proto-main2", main, vec![]).unwrap_err();
    assert!(e.contains("dep.proto"), "{e}");
    // The stored text of the main schema reads again (as the registry does to compare).
    let same = register(&r, "proto-main", main, vec![dep]).unwrap();
    assert_eq!(same, 2);
}
