#![no_main]
//! Any text as a JSON Schema (the bytes up to the first NUL are a referenced schema named `r.json`, the rest the schema;
//! a second NUL splits off another schema to compare with): never a panic; a schema that is read stores a text that
//! reads again as the same text, it reads its own data, and a comparison with any other schema ends.

use kafka_registry::format::{ParsedSchema, ResolvedRef, Role, SchemaType};
use kafka_registry::jsonschema::parse;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if kafka_registry::json::depth(text) > kafka_registry::json::MAX_SCHEMA_DEPTH {
        return;
    }
    let (reference, rest) = text.split_once('\0').unwrap_or(("", text));
    let (schema, other) = rest.split_once('\0').unwrap_or((rest, ""));
    let refs: Vec<ResolvedRef> = (!reference.is_empty())
        .then(|| ResolvedRef {
            name: "r.json".into(),
            subject: "r".into(),
            version: 1,
            kind: SchemaType::Json,
            text: reference.into(),
            direct: true,
            node: "r\01".into(),
            names: Vec::new(),
        })
        .into_iter()
        .collect();
    let Ok(s) = parse(schema, &refs) else {
        return;
    };
    let stored = s.canonical(false);
    let again = parse(&stored, &refs).expect("the stored text reads again");
    assert_eq!(again.canonical(false), stored);
    assert!(s.can_read(&again, Role::New, Role::Old).is_empty(), "a schema reads its own data");
    if let Ok(o) = parse(other, &refs) {
        let _ = o.can_read(&s, Role::New, Role::Old);
        let _ = s.can_read(&o, Role::Old, Role::New);
    }
});
