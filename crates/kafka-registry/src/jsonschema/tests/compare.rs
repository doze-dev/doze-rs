//! Comparisons the corpus does not reach: diamonds and recursion, and generated schemas.

use std::time::{Duration, Instant};

use super::super::*;
use super::{read, reference, refused};
use crate::format::Role;

/// Twenty-six levels, each naming the level below twice: 2^26 paths to the leaf.
fn diamond(leaf: &str) -> ParsedJsonSchema {
    let mut refs = vec![reference("l0.json", "l0", leaf, false, &[])];
    for i in 1..=26 {
        let below = format!("l{}", i - 1);
        let text = format!(
            r#"{{"type":"object","properties":{{"a":{{"$ref":"a{i}.json"}},"b":{{"$ref":"b{i}.json"}}}}}}"#
        );
        let names = [
            (format!("a{i}.json"), below.clone()),
            (format!("b{i}.json"), below),
        ];
        let names: Vec<(&str, &str)> = names
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        refs.push(reference(
            &format!("l{i}.json"),
            &format!("l{i}"),
            &text,
            false,
            &names,
        ));
    }
    if let Some(top) = refs.last_mut() {
        top.direct = true;
    }
    parse(r#"{"$ref":"l26.json"}"#, &refs).unwrap()
}

#[test]
fn a_diamond_of_references_compares_in_its_size_and_its_reasons_are_bounded() {
    let start = Instant::now();
    let (old, new) = (
        diamond(r#"{"type":"string"}"#),
        diamond(r#"{"type":"integer"}"#),
    );
    assert!(new.can_read(&new, Role::New, Role::Old).is_empty());
    let why = new.can_read(&old, Role::New, Role::Old);
    assert_eq!(why.len(), compat::MAX_REASONS);
    assert!(why.iter().all(|r| r.kind == "TYPE_CHANGED"));
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn a_recursive_schema_compares_with_itself_and_stops() {
    let tree = r##"{"type":"object","properties":{"v":{"type":"string"},"kids":{"type":"array","items":{"$ref":"#"}}}}"##;
    let s = read(tree);
    assert!(s.can_read(&s, Role::New, Role::Old).is_empty());
    let changed = read(&tree.replace("string", "integer"));
    assert!(!changed.can_read(&s, Role::New, Role::Old).is_empty());
    // A chain of references longer than a comparison may recurse is refused, not overflowed.
    let mut defs = Vec::new();
    for i in 0..300 {
        let next = if i == 0 {
            r#"{"type":"string"}"#.to_owned()
        } else {
            format!(r##"{{"items":{{"$ref":"#/definitions/d{}"}}}}"##, i - 1)
        };
        defs.push(format!(r#""d{i}":{next}"#));
    }
    let props: Vec<String> = (0..300)
        .map(|i| format!(r##""p{i}":{{"$ref":"#/definitions/d{i}"}}"##))
        .collect();
    let chain = format!(
        r#"{{"definitions":{{{}}},"properties":{{{}}}}}"#,
        defs.join(","),
        props.join(",")
    );
    assert!(refused(&chain).contains("too deeply"));
}

/// A small deterministic generator: no property-testing crate is in the build.
struct Gen(u64);

impl Gen {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }

    fn schema(&mut self, depth: u32) -> String {
        let leafs = [
            r#"{"type":"string"}"#,
            r#"{"type":"string","maxLength":5}"#,
            r#"{"type":"integer","minimum":0}"#,
            r#"{"type":"number","multipleOf":0.5}"#,
            r#"{"enum":["a","b"]}"#,
            r#"{"const":1}"#,
            "true",
            "false",
            r#"{"type":["string","null"]}"#,
            r#"{"type":"boolean","default":true}"#,
        ];
        if depth == 0 || self.next(3) == 0 {
            return leafs
                .get(self.next(leafs.len() as u64) as usize)
                .copied()
                .unwrap_or("{}")
                .to_owned();
        }
        match self.next(4) {
            0 => format!(
                r#"{{"type":"array","items":{},"maxItems":{}}}"#,
                self.schema(depth - 1),
                self.next(4)
            ),
            1 => format!(
                r#"{{"anyOf":[{},{}]}}"#,
                self.schema(depth - 1),
                self.schema(depth - 1)
            ),
            2 => format!(
                r#"{{"allOf":[{}],"not":{}}}"#,
                self.schema(depth - 1),
                self.schema(depth - 1)
            ),
            _ => {
                let n = self.next(4);
                let props: Vec<String> = (0..n)
                    .map(|i| format!(r#""p{}":{}"#, self.next(6) * 10 + i, self.schema(depth - 1)))
                    .collect();
                let extra = [
                    "",
                    r#","additionalProperties":false"#,
                    r#","additionalProperties":{"type":"string"}"#,
                    r#","required":["p1"]"#,
                ];
                let e = extra.get(self.next(4) as usize).copied().unwrap_or("");
                format!(
                    r#"{{"type":"object","properties":{{{}}}{e}}}"#,
                    props.join(",")
                )
            }
        }
    }
}

#[test]
fn any_schema_reads_its_own_data_and_its_stored_text_reads_back_the_same() {
    let mut g = Gen(7);
    for _ in 0..400 {
        let (a, b) = (g.schema(4), g.schema(4));
        let (sa, sb) = (read(&a), read(&b));
        assert!(sa.can_read(&sa, Role::New, Role::Old).is_empty(), "{a}");
        assert_eq!(
            read(&sa.canonical(false)).canonical(false),
            sa.canonical(false)
        );
        // Every reason names a breaking change, with a path into the schema and Confluent's sentence shape.
        for r in sb.can_read(&sa, Role::New, Role::Old) {
            assert!(
                compat::describe(&r.kind, "#", "new", "old").contains("new")
                    || r.kind == "SCHEMA_ADDED",
                "{r:?}"
            );
            assert!(
                r.path.starts_with('#') && r.text.starts_with("{errorType:"),
                "{r:?}"
            );
        }
    }
}
