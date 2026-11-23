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
    let again = match parse(&stored, &refs) {
        Ok(again) => again,
        // As in Confluent 7.7.1: a BigDecimal whose adjusted exponent is one past an `int` (`12e2147483647`) is held, and
        // written `1.2E+2147483648`, which is no BigDecimal and reads again as an infinite double (recorded by
        // tools/registry-diff, `json-schema-huge-numbers`; REG-50). The registry stores that text and refuses a comparison
        // with it in Confluent's words (`prepare::unreadable`). No other stored text may fail to read again.
        Err(e) if past_an_int(&stored, &e) => return,
        Err(e) => panic!("the stored text reads again: {stored} gave {e}"),
    };
    let restored = again.canonical(false);
    let stable = (restored != stored).then(|| {
        // As in Confluent: a number too small for a BigDecimal is Java's double zero, written `-0.0` when negative, and
        // `-0.0` read again is a BigDecimal, which has no negative zero (`0.0`). One more reading changes nothing.
        assert!(stored.contains("-0.0"), "{stored} was stored, {restored} once read again");
        let third = parse(&restored, &refs).expect("the text stored again reads again");
        assert_eq!(third.canonical(false), restored);
        third
    });
    // A `-0.0` constant is not the `0.0` it reads again as, so then the text that reads the same is checked against itself.
    let (old, new) = match &stable {
        Some(third) => (&again, third),
        None => (&s, &again),
    };
    assert!(old.can_read(new, Role::New, Role::Old).is_empty(), "a schema reads its own data");
    if let Ok(o) = parse(other, &refs) {
        let _ = o.can_read(&s, Role::New, Role::Old);
        let _ = s.can_read(&o, Role::Old, Role::New);
    }
});

/// Whether `e` refuses a number past a double and `stored` writes one with an exponent past an `int`, as `1.2E+2147483648`
/// (the refusal cuts a long number, so the exponent is read from the stored text).
fn past_an_int(stored: &str, e: &str) -> bool {
    e.ends_with(" is past the largest number a Java double holds")
        && stored.split("E+").skip(1).any(|rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<i64>().is_ok_and(|exp| i32::try_from(exp).is_err())
        })
}
