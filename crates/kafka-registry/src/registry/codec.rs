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
        let p = self
            .parse_stored(&s, id)
            .map_err(|e| RegistryError::invalid_schema(format!("schema {id} cannot be read: {e}")))?;
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
            .register(subject, &SchemaInput::of(kind, text), false, &Origin::default())
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
        let e = c.encode(r#"{"id":"x"}"#).unwrap_err();
        assert_eq!(e, "id: expected a number, not \"x\"");
        let e = c.encode(r#"{"id":1,"state":"GONE","lines":[],"tags":{},"code":"AB","blob":"","f":1}"#).unwrap_err();
        assert!(e.starts_with("state: \"GONE\" is not a symbol of enum shop.State"), "{e}");
        let e = c.encode(r#"{"id":1,"state":"NEW","tags":{},"code":"AB","blob":"","f":1}"#).unwrap_err();
        assert_eq!(e, "field \"lines\" is missing and has no default");
        assert!(c.encode("{").unwrap_err().starts_with("not JSON"));
    }

    #[test]
    fn hostile_avro_bytes_are_refused_before_anything_is_allocated() {
        let r = registry();
        let id = reg(&r, "s", SchemaType::Avro, r#"{"type":"array","items":"null"}"#);
        let c = r.codec(id).unwrap();
        // A block of 2^62 nulls: no bytes each, so the budget refuses it.
        let mut v = crate::wire::header(id).to_vec();
        v.extend([0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]);
        let e = c.decode(&v).unwrap_err();
        assert!(e.contains("more than"), "{e}");
        let id = reg(&r, "t", SchemaType::Avro, r#""string""#);
        let c = r.codec(id).unwrap();
        let mut v = crate::wire::header(id).to_vec();
        v.extend([0xfe, 0xff, 0xff, 0xff, 0x0f, b'a']);
        let e = c.decode(&v).unwrap_err();
        assert!(e.starts_with("the payload does not match the schema (a string length of"), "{e}");
        assert_eq!(c.decode(b"plain").unwrap_err(), "not in the registry's wire format");
        let other = crate::wire::header(id + 7);
        assert!(c.decode(&other).unwrap_err().contains("names schema id"));
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
        assert!(c.decode(&v).unwrap_err().contains("levels deep"));
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
            (r#"{"id":1,"x":1}"#, "#: property \"x\" is not allowed (additionalProperties is false)"),
            (r#"{"id":1,"tags":["a","a"]}"#, "#/tags: its items are not unique"),
            (r#"{"id":1,"tags":[1]}"#, "#/tags/0: expected a string, not a number"),
            (r#"{"id":1,"kind":"c"}"#, "#/kind: \"c\" is not one of its enum values"),
        ] {
            assert_eq!(c.encode(bad).unwrap_err(), why, "{bad}");
        }
        let mut stored = crate::wire::header(id).to_vec();
        stored.extend(br#"{"id":0}"#);
        let d = c.decode(&stored).unwrap();
        assert_eq!(d.mismatch.as_deref(), Some("#/id: 0 is under its minimum"));
        stored.truncate(5);
        stored.extend(b"not json");
        assert!(c.decode(&stored).unwrap_err().contains("not JSON"));
    }

    #[test]
    fn an_unknown_id_is_confluents_40403() {
        let r = registry();
        assert_eq!(r.codec(9).unwrap_err().error_code, 40403);
        assert_eq!(r.codec_for("nope", "latest").unwrap_err().error_code, 40401);
    }
}
