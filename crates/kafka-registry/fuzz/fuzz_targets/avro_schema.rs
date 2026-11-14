#![no_main]
//! Any text as an Avro schema (the bytes up to the first NUL are a referenced schema, the rest the schema): never a
//! panic; a schema that is read writes a stored text that reads again as the same stored text, and it can read itself.

use kafka_registry::avro::parse;
use kafka_registry::format::{ParsedSchema, ResolvedRef, Role, SchemaType};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if kafka_registry::json::depth(text) > kafka_registry::json::MAX_SCHEMA_DEPTH {
        return;
    }
    let (reference, schema) = text.split_once('\0').unwrap_or(("", text));
    let refs: Vec<ResolvedRef> = (!reference.is_empty())
        .then(|| ResolvedRef {
            name: "r".into(),
            subject: "r".into(),
            version: 1,
            kind: SchemaType::Avro,
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
    let _ = s.canonical(true);
    assert!(s.can_read(&again, Role::New, Role::Old).is_empty(), "a schema reads its own data");
});
