//! The record codec: a record value in the registry's wire format ([`crate::wire`]) read as the JSON it means, and JSON a
//! person typed written as the value a Confluent serializer would have sent. The console decodes records with it and
//! encodes what its Produce dialog sends; it never goes over HTTP to the registry it lives beside.
//!
//! Each language reads and writes its own payloads ([`crate::format::ParsedSchema::decode`] and `encode`): Avro binary,
//! Protobuf binary after its message index, JSON text checked against its JSON Schema. The bytes are whatever any producer
//! wrote, so every length and count in them is checked against what is left before anything is allocated, and a
//! [`Budget`] bounds the items and the nesting one value may make (the guard the Go edition kept in `avroguard.go`).

use crate::format::{ParsedSchema, SchemaType};
use crate::json::{Json, write_str};
use crate::wire;

/// The most items (fields, array and map entries, union branches) one value may decode to.
pub const MAX_ITEMS: usize = 1 << 20;
/// The deepest one value may nest.
pub const MAX_DEPTH: usize = 64;

/// Why a record value cannot be read with a schema.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error(transparent)]
    Wire(#[from] wire::WireError),
    #[error("the value names schema id {named}, not {ours}")]
    OtherSchema { named: u32, ours: u32 },
    /// The payload does not fit the schema: where and why, in the words of the language's reader.
    #[error("the payload does not match the schema ({0})")]
    Mismatch(String),
}

/// Why JSON text cannot be written as a value of a schema.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EncodeError {
    #[error("not JSON: {0}")]
    NotJson(#[from] crate::json::JsonError),
    /// The value does not fit the schema: where and why, in the words of the language's writer.
    #[error("{0}")]
    Mismatch(String),
}

/// What one decode may still spend.
#[derive(Debug)]
pub struct Budget {
    items: usize,
    depth: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            items: MAX_ITEMS,
            depth: MAX_DEPTH,
        }
    }
}

impl Budget {
    /// Spends `n` items.
    ///
    /// # Errors
    /// More items than [`MAX_ITEMS`].
    pub fn take(&mut self, n: u64) -> Result<(), String> {
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        self.items = self.items.checked_sub(n).ok_or_else(|| {
            format!("it would make more than {MAX_ITEMS} items, more than the console decodes")
        })?;
        Ok(())
    }

    /// One level deeper, for the length of `f`.
    ///
    /// # Errors
    /// Deeper than [`MAX_DEPTH`], or whatever `f` says.
    pub fn deeper<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        if self.depth == 0 {
            return Err(format!("it nests more than {MAX_DEPTH} levels deep"));
        }
        self.depth -= 1;
        let out = f(self);
        self.depth += 1;
        out
    }
}

/// A payload read as JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub json: Json,
    /// The Protobuf message the value's index names (`orders.v1.Order`).
    pub message: Option<String>,
    /// Why a value that reads (JSON text) does not fit its JSON Schema.
    pub mismatch: Option<String>,
}

/// One schema, ready to read and write payloads.
#[derive(Debug)]
pub struct Codec {
    pub(crate) id: u32,
    pub(crate) kind: SchemaType,
    pub(crate) parsed: Box<dyn ParsedSchema>,
}

impl Codec {
    /// A codec over a schema a language has read (a test's, or the registry's [`crate::Registry::codec`]).
    #[must_use]
    pub fn new(id: u32, kind: SchemaType, parsed: Box<dyn ParsedSchema>) -> Self {
        Self { id, kind, parsed }
    }

    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }

    #[must_use]
    pub fn kind(&self) -> SchemaType {
        self.kind
    }

    /// Reads a whole record value, header included.
    ///
    /// # Errors
    /// Not in the wire format, another schema's id, or a payload that does not fit the schema: the reason in words.
    // fuzz:parser codec
    pub fn decode(&self, value: &[u8]) -> Result<Decoded, DecodeError> {
        let (id, body) = wire::split(value)?;
        if id != self.id {
            return Err(DecodeError::OtherSchema {
                named: id,
                ours: self.id,
            });
        }
        self.parsed.decode(body).map_err(DecodeError::Mismatch)
    }

    /// A value of the schema's shape, every field at its zero value.
    #[must_use]
    pub fn sample(&self) -> Json {
        self.parsed.sample()
    }

    /// Writes JSON text as a record value: the header, then the payload.
    ///
    /// # Errors
    /// The text is not JSON, or does not fit the schema: the reason in words.
    pub fn encode(&self, text: &str) -> Result<Vec<u8>, EncodeError> {
        let value = Json::parse(text)?;
        let mut out = wire::header(self.id).to_vec();
        out.extend(self.parsed.encode(&value).map_err(EncodeError::Mismatch)?);
        Ok(out)
    }
}

/// `j` laid out for reading: two spaces a level, one key or item a line, an empty object or array as `{}` or `[]`.
#[must_use]
pub fn pretty(j: &Json) -> String {
    let mut out = String::new();
    lay_out(j, 0, &mut out);
    out
}

fn indent(out: &mut String, level: usize) {
    out.push('\n');
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn lay_out(j: &Json, level: usize, out: &mut String) {
    match j {
        Json::Object(m) if !m.is_empty() => {
            out.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                indent(out, level + 1);
                write_str(out, k);
                out.push_str(": ");
                lay_out(v, level + 1, out);
            }
            indent(out, level);
            out.push('}');
        }
        Json::Array(items) if !items.is_empty() => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                indent(out, level + 1);
                lay_out(v, level + 1, out);
            }
            indent(out, level);
            out.push(']');
        }
        other => other.write(out),
    }
}

/// A JSON number for an `f64`: NaN and the infinities, which JSON has no number for, as strings (as the Go edition did).
#[must_use]
pub fn float(f: f64) -> Json {
    if f.is_finite() {
        Json::Raw(format!("{f:?}").trim_end_matches(".0").to_owned())
    } else {
        Json::Str(
            if f.is_nan() {
                "NaN"
            } else if f > 0.0 {
                "Infinity"
            } else {
                "-Infinity"
            }
            .to_owned(),
        )
    }
}

/// Bytes as Avro's JSON encoding writes them: each byte as the character with that code.
#[must_use]
pub fn bytes_text(b: &[u8]) -> Json {
    Json::Str(b.iter().map(|&c| char::from(c)).collect())
}

/// The bytes a string stands for: Avro's one character a byte (codes 0 to 255). (A base64 fallback once sat behind this;
/// it could never be reached, as base64 is all ASCII and so always read here first.)
///
/// # Errors
/// A character past 255.
pub fn text_bytes(s: &str) -> Result<Vec<u8>, String> {
    if let Some(b) = s
        .chars()
        .map(|c| u8::try_from(u32::from(c)).ok())
        .collect::<Option<Vec<u8>>>()
    {
        return Ok(b);
    }
    Err(format!(
        "{s:?} is not bytes: characters must be 0 to 255 (or the value base64)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_laid_out_two_spaces_a_level_in_its_own_order() {
        let j = Json::parse(r#"{"b":1,"a":[true,{}],"c":{"d":"x"},"e":[]}"#).unwrap();
        assert_eq!(
            pretty(&j),
            "{\n  \"b\": 1,\n  \"a\": [\n    true,\n    {}\n  ],\n  \"c\": {\n    \"d\": \"x\"\n  },\n  \"e\": []\n}"
        );
    }

    #[test]
    fn bytes_are_characters_or_base64() {
        assert_eq!(text_bytes("\u{0}\u{ff}a").unwrap(), vec![0, 255, b'a']);
        assert_eq!(text_bytes("aGk=").unwrap(), b"aGk=".to_vec());
        assert_eq!(text_bytes("€€€€").map_err(|_| ()), Err(()));
        assert_eq!(bytes_text(&[104, 0xe9]), Json::Str("hé".into()));
    }

    #[test]
    fn floats_are_numbers_and_the_others_are_words() {
        assert_eq!(float(1.5), Json::Raw("1.5".into()));
        assert_eq!(float(3.0), Json::Raw("3".into()));
        assert_eq!(float(f64::NAN), Json::Str("NaN".into()));
        assert_eq!(float(f64::INFINITY), Json::Str("Infinity".into()));
    }

    /// The fuzzer's find (`crash-4684f40f…`, input `00 1a 00`): an empty packed run read as `"c": []`, which wrote back
    /// as nothing and read again as `{}`.
    #[test]
    fn an_empty_packed_run_reads_as_the_field_left_out_and_round_trips() {
        let text = "syntax = \"proto3\";\nmessage M { int64 a = 1; repeated sint32 c = 3; }\n";
        let c = Codec::new(
            3,
            SchemaType::Protobuf,
            Box::new(crate::protobuf::parse(text, &[]).unwrap()),
        );
        let mut value = wire::header(3).to_vec();
        value.extend_from_slice(&[0, 0x1a, 0]);
        let d = c.decode(&value).unwrap();
        assert_eq!(d.json.to_text(), "{}");
        let again = c.encode(&d.json.to_text()).unwrap();
        assert_eq!(c.decode(&again).unwrap().json.to_text(), "{}");
    }

    #[test]
    fn the_budget_bounds_items_and_depth() {
        let mut b = Budget::default();
        assert!(b.take(MAX_ITEMS as u64).is_ok());
        assert!(b.take(1).is_err());
        let mut b = Budget::default();
        fn dig(b: &mut Budget, n: usize) -> Result<(), String> {
            if n == 0 {
                Ok(())
            } else {
                b.deeper(|b| dig(b, n - 1))
            }
        }
        assert!(dig(&mut b, MAX_DEPTH).is_ok());
        assert!(dig(&mut b, MAX_DEPTH + 1).is_err());
    }
}
