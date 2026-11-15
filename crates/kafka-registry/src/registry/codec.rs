//! A stored schema as a [`Codec`]: what the console decodes a record and encodes a value with, in the same process.

use super::Registry;
use crate::codec::Codec;
use crate::error::RegistryError;

impl Registry {
    /// The codec of schema `id`, its references followed.
    ///
    /// # Errors
    /// No schema has the id (Confluent's 40403), or it cannot be read again (its language is not in this build).
    pub fn codec(&self, id: u32) -> Result<Codec, RegistryError> {
        let s = self.lock();
        if !s.schemas.contains_key(&id) {
            return Err(RegistryError::schema_id_not_found(id));
        }
        let p = self.parse_stored(&s, id).map_err(|e| {
            RegistryError::invalid_schema(format!("schema {id} cannot be read: {e}"))
        })?;
        Ok(Codec::new(id, p.kind, p.parsed))
    }

    /// The codec of a version of a subject (`latest`, or a number), and the version's number.
    ///
    /// # Errors
    /// An invalid version, no such subject or version (a soft-deleted one included), or as [`Registry::codec`].
    pub fn codec_for(&self, subject: &str, version: &str) -> Result<(Codec, u32), RegistryError> {
        let (id, number) = {
            let s = self.lock();
            let v = s.version_of(subject, version, false)?;
            (v.rec.id, v.number)
        };
        Ok((self.codec(id)?, number))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Origin, RegisterResult, SchemaInput};
    use super::*;
    use crate::codec::pretty;
    use crate::format::SchemaType;
    use crate::store::MemoryStorage;

    const ORDER: &str = r#"{"type":"record","name":"Order","namespace":"shop","fields":[
        {"name":"id","type":"long"},
        {"name":"note","type":["null","string"],"default":null},
        {"name":"state","type":{"type":"enum","name":"State","symbols":["NEW","PAID"]}},
        {"name":"lines","type":{"type":"array","items":"double"}},
        {"name":"tags","type":{"type":"map","values":"int"}},
        {"name":"code","type":{"type":"fixed","name":"Code","size":2}},
        {"name":"ok","type":"boolean","default":true},
        {"name":"blob","type":"bytes"},
        {"name":"f","type":"float"}]}"#;

    fn registry() -> Registry {
        Registry::open(Box::new(MemoryStorage::default())).unwrap()
    }

    fn reg(r: &Registry, subject: &str, kind: SchemaType, text: &str) -> u32 {
        match r
            .register(
                subject,
                &SchemaInput::of(kind, text),
                false,
                &Origin::default(),
            )
            .unwrap()
        {
            RegisterResult::Id(id) => id,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn avro_values_round_trip_with_tagged_unions_defaults_and_bytes_as_characters() {
        let r = registry();
        let id = reg(&r, "orders-value", SchemaType::Avro, ORDER);
        let (c, v) = r.codec_for("orders-value", "latest").unwrap();
        assert_eq!((c.id(), v), (id, 1));
        let typed = r#"{"id":42,"note":"rush","state":"PAID","lines":[1.5,2],"tags":{"a":1},"code":"AB","blob":"hi","f":0.5}"#;
        let bytes = c.encode(typed).unwrap();
        assert_eq!(&bytes[..5], &[0, 0, 0, 0, u8::try_from(id).unwrap()]);
        let d = c.decode(&bytes).unwrap();
        assert_eq!(
            d.json.to_text(),
            r#"{"id":42,"note":{"string":"rush"},"state":"PAID","lines":[1.5,2],"tags":{"a":1},"code":"AB","ok":true,"blob":"hi","f":0.5}"#
        );
        // What was decoded encodes to the same bytes: a record read here can be sent as it stands.
        assert_eq!(c.encode(&d.json.to_text()).unwrap(), bytes);
        assert!(pretty(&d.json).starts_with("{\n  \"id\": 42,"));
    }

    #[test]
    fn avro_values_that_do_not_fit_say_where() {
        let r = registry();
        reg(&r, "orders-value", SchemaType::Avro, ORDER);
        let (c, _) = r.codec_for("orders-value", "1").unwrap();
        let e = c.encode(r#"{"id":"x"}"#).unwrap_err().to_string();
        assert_eq!(e, "id: expected a number, not \"x\"");
        let e = c
            .encode(r#"{"id":1,"state":"GONE","lines":[],"tags":{},"code":"AB","blob":"","f":1}"#)
            .unwrap_err()
            .to_string();
        assert!(
            e.starts_with("state: \"GONE\" is not a symbol of enum shop.State"),
            "{e}"
        );
        let e = c
            .encode(r#"{"id":1,"state":"NEW","tags":{},"code":"AB","blob":"","f":1}"#)
            .unwrap_err()
            .to_string();
        assert_eq!(e, "field \"lines\" is missing and has no default");
        assert!(
            c.encode("{")
                .unwrap_err()
                .to_string()
                .starts_with("not JSON")
        );
    }

    #[test]
    fn hostile_avro_bytes_are_refused_before_anything_is_allocated() {
        let r = registry();
        let id = reg(
            &r,
            "s",
            SchemaType::Avro,
            r#"{"type":"array","items":"null"}"#,
        );
        let c = r.codec(id).unwrap();
        // A block of 2^62 nulls: no bytes each, so the budget refuses it.
        let mut v = crate::wire::header(id).to_vec();
        v.extend([0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]);
        let e = c.decode(&v).unwrap_err().to_string();
        assert!(e.contains("more than"), "{e}");
        let id = reg(&r, "t", SchemaType::Avro, r#""string""#);
        let c = r.codec(id).unwrap();
        let mut v = crate::wire::header(id).to_vec();
        v.extend([0xfe, 0xff, 0xff, 0xff, 0x0f, b'a']);
        let e = c.decode(&v).unwrap_err().to_string();
        assert!(
            e.starts_with("the payload does not match the schema (a string length of"),
            "{e}"
        );
        assert_eq!(
            c.decode(b"plain").unwrap_err().to_string(),
            "not in the registry's wire format"
        );
        let other = crate::wire::header(id + 7);
        assert!(
            c.decode(&other)
                .unwrap_err()
                .to_string()
                .contains("names schema id")
        );
    }

    #[test]
    fn a_recursive_avro_type_is_bounded_in_depth() {
        let r = registry();
        let id = reg(
            &r,
            "list",
            SchemaType::Avro,
            r#"{"type":"record","name":"Node","fields":[{"name":"next","type":["null","Node"]}]}"#,
        );
        let c = r.codec(id).unwrap();
        let mut v = crate::wire::header(id).to_vec();
        v.extend(std::iter::repeat_n(2u8, 500));
        v.push(0);
        assert!(
            c.decode(&v)
                .unwrap_err()
                .to_string()
                .contains("levels deep")
        );
    }

    #[test]
    fn json_schema_values_are_checked_and_shown_as_they_are() {
        let r = registry();
        let s = r#"{"type":"object","properties":{"id":{"type":"integer","minimum":1},"tags":{"type":"array","items":{"type":"string"},"uniqueItems":true},"kind":{"enum":["a","b"]}},"required":["id"],"additionalProperties":false}"#;
        let id = reg(&r, "j-value", SchemaType::Json, s);
        let c = r.codec(id).unwrap();
        let bytes = c.encode(r#"{"id": 3, "tags": ["x"]}"#).unwrap();
        assert_eq!(&bytes[5..], br#"{"id":3,"tags":["x"]}"#);
        let d = c.decode(&bytes).unwrap();
        assert_eq!(d.mismatch, None);
        for (bad, why) in [
            (r#"{"tags":[]}"#, "#: required property \"id\" is missing"),
            (r#"{"id":0}"#, "#/id: 0 is under its minimum"),
            (r#"{"id":1.5}"#, "#/id: expected an integer, not 1.5"),
            (
                r#"{"id":1,"x":1}"#,
                "#: property \"x\" is not allowed (additionalProperties is false)",
            ),
            (
                r#"{"id":1,"tags":["a","a"]}"#,
                "#/tags: its items are not unique",
            ),
            (
                r#"{"id":1,"tags":[1]}"#,
                "#/tags/0: expected a string, not a number",
            ),
            (
                r#"{"id":1,"kind":"c"}"#,
                "#/kind: \"c\" is not one of its enum values",
            ),
        ] {
            assert_eq!(c.encode(bad).unwrap_err().to_string(), why, "{bad}");
        }
        let mut stored = crate::wire::header(id).to_vec();
        stored.extend(br#"{"id":0}"#);
        let d = c.decode(&stored).unwrap();
        assert_eq!(d.mismatch.as_deref(), Some("#/id: 0 is under its minimum"));
        stored.truncate(5);
        stored.extend(b"not json");
        assert!(
            c.decode(&stored)
                .unwrap_err()
                .to_string()
                .contains("not JSON")
        );
    }

    const PROTO: &str = r#"syntax = "proto3";
package shop.v1;
message Order {
  int64 order_id = 1;
  string note = 2;
  repeated int32 qty = 3;
  State state = 4;
  map<string, int32> tags = 5;
  Line line = 6;
  bytes blob = 7;
  sint32 delta = 8;
  message Line { string sku = 1; double price = 2; }
}
enum State { NEW = 0; PAID = 1; }
message Other { bool ok = 1; }
"#;

    #[test]
    fn protobuf_values_read_as_protojson_by_their_message_index() {
        let r = registry();
        let id = reg(&r, "orders-value", SchemaType::Protobuf, PROTO);
        let c = r.codec(id).unwrap();
        // What a serializer writes: header, index [0] as a single 0, then order_id 150 (08 96 01) and note "hi".
        let mut v = crate::wire::header(id).to_vec();
        v.extend([
            0, 0x08, 0x96, 0x01, 0x12, 0x02, b'h', b'i', 0x98, 0x06, 0x01,
        ]);
        let d = c.decode(&v).unwrap();
        assert_eq!(d.json.to_text(), r#"{"orderId":"150","note":"hi"}"#);
        assert_eq!(d.message.as_deref(), Some("shop.v1.Order"));
        let typed = r#"{"order_id":"42","qty":[1,2,300],"state":"PAID","tags":{"a":7},"line":{"sku":"x","price":2.5},"blob":"aGk=","delta":-3}"#;
        let bytes = c.encode(typed).unwrap();
        assert_eq!(bytes[5], 0, "index [0] as one zero");
        let d = c.decode(&bytes).unwrap();
        assert_eq!(
            d.json.to_text(),
            r#"{"orderId":"42","qty":[1,2,300],"state":"PAID","tags":{"a":7},"line":{"sku":"x","price":2.5},"blob":"aGk=","delta":-3}"#
        );
        assert_eq!(c.encode(&d.json.to_text()).unwrap(), bytes);
        // Index [1] is the second message of the file; [0, 0] the second nested in the first.
        let mut v = crate::wire::header(id).to_vec();
        v.extend([2, 2, 0x08, 0x01]);
        let d = c.decode(&v).unwrap();
        assert_eq!(
            (d.json.to_text().as_str(), d.message.as_deref()),
            (r#"{"ok":true}"#, Some("shop.v1.Other"))
        );
        let mut v = crate::wire::header(id).to_vec();
        // The map's entry is nested first, where protoc puts it: Line is [0, 1].
        v.extend([4, 0, 2, 0x0a, 0x01, b'z']);
        assert_eq!(
            c.decode(&v).unwrap().message.as_deref(),
            Some("shop.v1.Order.Line")
        );
        let mut v = crate::wire::header(id).to_vec();
        v.extend([2, 18]);
        assert!(
            c.decode(&v)
                .unwrap_err()
                .to_string()
                .contains("outside the schema")
        );
        assert_eq!(
            c.encode(r#"{"nope":1}"#).unwrap_err().to_string(),
            "shop.v1.Order has no field \"nope\""
        );
        assert!(
            c.encode(r#"{"state":"GONE"}"#)
                .unwrap_err()
                .to_string()
                .starts_with("state: \"GONE\" is not a value")
        );
    }

    #[test]
    fn hostile_protobuf_bytes_are_refused() {
        let r = registry();
        let id = reg(&r, "p", SchemaType::Protobuf, PROTO);
        let c = r.codec(id).unwrap();
        // A length far past the end; a group that never ends.
        let mut v = crate::wire::header(id).to_vec();
        v.extend([0, 0x12, 0xff, 0xff, 0xff, 0x0f]);
        assert!(c.decode(&v).unwrap_err().to_string().contains("are left"));
        let mut v = crate::wire::header(id).to_vec();
        v.extend([0, 0x4b]);
        v.extend(std::iter::repeat_n(0x4bu8, 300));
        assert!(c.decode(&v).is_err());
    }

    #[test]
    fn an_unknown_id_is_confluents_40403() {
        let r = registry();
        assert_eq!(r.codec(9).unwrap_err().error_code, 40403);
        assert_eq!(r.codec_for("nope", "latest").unwrap_err().error_code, 40401);
    }
}
