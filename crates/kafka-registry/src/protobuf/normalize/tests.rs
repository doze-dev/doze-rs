//! The normalizer's own pieces.

use super::*;
use crate::format::ParsedSchema;
use crate::protobuf::parse;

#[test]
fn numbers_are_written_as_javas_decimal_format_writes_what_commons_lang_reads() {
    for (raw, want) in [
        ("1", "1"),
        ("-0", "0"),
        ("0x10", "16"),
        ("-0x10", "-16"),
        ("010", "8"),
        ("2.50", "2.5"),
        ("1e3", "1000"),
        ("1.23456", "1.235"),
        ("0.0625", "0.062"),
        ("0.0005", "0.001"),
        ("-0.0001", "-0"),
        ("1.1", "1.1"),
        ("inf", "inf"),
        ("-inf", "-inf"),
        ("nan", "nan"),
        (
            "123456789012345678901234567890",
            "123456789012345678901234567890",
        ),
        ("0.1234567890123456789", "0.123"),
        // commons-lang reads it as a float, which DecimalFormat writes as its double.
        ("1e20", "100000002004087730000"),
    ] {
        assert_eq!(java_format_number(raw), want, "{raw}");
    }
}

#[test]
fn indenting_drops_trailing_empty_lines_as_java_splits() {
    let mut s = String::new();
    indented(&mut s, "a\n\nb\n");
    assert_eq!(s, "  a\n  \n  b\n");
    let mut s = String::new();
    indented(&mut s, "x");
    assert_eq!(s, "  x\n");
}

#[test]
fn strings_are_read_as_wire_reads_them() {
    assert_eq!(wire_unquote(r#""a\x41\101\?\q" 'b'"#), "aAA?qb");
    assert_eq!(escape("a\"'\\\n\u{1}"), "a\\\"\\'\\\\\\n\u{1}");
}

/// Recorded from cp-schema-registry 7.7.1 and 8.3.2 (tools/registry-diff, protobuf-normalize/names): every name qualified
/// from its scope, rpc options sorted, fields by number. The normalized text reads back as itself.
#[test]
fn a_schema_normalizes_as_confluent_normalizes_it() {
    let text = "syntax = \"proto3\";
package a.b;
message Outer {
  message Inner { message Leaf { int32 x = 1; } Leaf leaf = 1; }
  Inner.Leaf direct = 1;
  .a.b.Outer.Inner absolute = 2;
  b.Outer up = 3;
  repeated Inner inners = 4;
}
message Other { Outer.Inner.Leaf l = 1; a.b.Other self = 2; }
service S {
  rpc R (Outer) returns (b.Other) { option idempotency_level = NO_SIDE_EFFECTS; option deprecated = true; }
  rpc Q (stream .a.b.Other) returns (Other);
}

";
    let want = "syntax = \"proto3\";
package a.b;

message Outer {
  .a.b.Outer.Inner.Leaf direct = 1;
  .a.b.Outer.Inner absolute = 2;
  .a.b.Outer up = 3;
  repeated .a.b.Outer.Inner inners = 4;

  message Inner {
    .a.b.Outer.Inner.Leaf leaf = 1;
  
    message Leaf {
      int32 x = 1;
    }
  }
}
message Other {
  .a.b.Outer.Inner.Leaf l = 1;
  .a.b.Other self = 2;
}

service S {
  rpc R (.a.b.Outer) returns (.a.b.Other) {
    option deprecated = true;
    option idempotency_level = NO_SIDE_EFFECTS;
  };
  rpc Q (stream .a.b.Other) returns (.a.b.Other);
}
";
    let got = parse(text, &[]).unwrap().canonical(true);
    assert_eq!(got, want);
    assert_eq!(parse(&got, &[]).unwrap().canonical(true), want);
    // proto2 loses its syntax line.
    let p2 = parse(
        "syntax = \"proto2\"; message A { optional int32 a = 1; }",
        &[],
    )
    .unwrap();
    assert_eq!(
        p2.canonical(true),
        "\nmessage A {\n  optional int32 a = 1;\n}\n"
    );
}
