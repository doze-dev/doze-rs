#![no_main]
//! Any record value read with an Avro, a JSON Schema and a Protobuf schema: never a panic, never unbounded work; a value that
//! reads writes back to bytes that read as the same JSON.

use std::sync::OnceLock;

use kafka_registry::codec::Codec;
use kafka_registry::format::SchemaType;
use kafka_registry::wire::header;
use libfuzzer_sys::fuzz_target;

const AVRO: &str = r#"{"type":"record","name":"R","fields":[{"name":"a","type":["null","long","string"]},{"name":"b","type":{"type":"array","items":{"type":"map","values":"double"}}},{"name":"c","type":{"type":"enum","name":"E","symbols":["X","Y"]}},{"name":"d","type":{"type":"fixed","name":"F","size":2}},{"name":"n","type":["null","R"]}]}"#;
const JSON: &str = r#"{"type":"object","properties":{"a":{"type":"integer"},"b":{"type":"array","items":{"type":"string"}}},"required":["a"]}"#;
const PROTO: &str = "syntax = \"proto3\";\nmessage M { int64 a = 1; string b = 2; repeated sint32 c = 3; map<string, M> d = 4; bytes e = 5; N f = 6; message N { double x = 1; } }\n";

fn codecs() -> &'static [Codec; 3] {
    static C: OnceLock<[Codec; 3]> = OnceLock::new();
    C.get_or_init(|| {
        let one = |id, kind: SchemaType, text: &str| {
            let parsed = match kind {
                SchemaType::Avro => Box::new(kafka_registry::avro::parse(text, &[]).expect("avro")) as Box<dyn kafka_registry::format::ParsedSchema>,
                SchemaType::Json => Box::new(kafka_registry::jsonschema::parse(text, &[]).expect("json")),
                SchemaType::Protobuf => Box::new(kafka_registry::protobuf::parse(text, &[]).expect("proto")),
            };
            Codec::new(id, kind, parsed)
        };
        [one(1, SchemaType::Avro, AVRO), one(2, SchemaType::Json, JSON), one(3, SchemaType::Protobuf, PROTO)]
    })
}

fuzz_target!(|data: &[u8]| {
    for c in codecs() {
        let mut value = header(c.id()).to_vec();
        value.extend_from_slice(data);
        if let Ok(d) = c.decode(&value) {
            let text = d.json.to_text();
            if c.kind() != SchemaType::Json && d.message.as_deref().is_none_or(|m| m == "M") {
                let again = c.encode(&text).expect("a decoded value encodes");
                let back = c.decode(&again).expect("an encoded value decodes");
                assert_eq!(back.json.to_text(), text);
            }
        }
    }
});
