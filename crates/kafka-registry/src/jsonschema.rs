//! JSON Schema, read and compared as Confluent's registry does, written here (no validator library: the two
//! candidates cost 1.3 to 1.9 MB; docs/port/phase10.md, "Weight first").
//!
//! - `doc`: the schema and the schemas its references reach, each a document with its own names and draft.
//! - `check`: whether each is a valid schema of its draft (the meta-schema, keyword by keyword; references resolve).
//! - `node`: the form Confluent compares, references followed.
//! - `compat`: Confluent's structural compatibility rules and sentences.
//!
//! The stored text is the schema as written, compacted (Confluent's Jackson writes it back without spaces).

mod check;
mod compat;
mod doc;
mod node;
mod number;

use std::any::Any;

use crate::format::{CompatReason, ParsedSchema, ResolvedRef, Role, SchemaFormat, SchemaType};
use crate::json::Json;

/// The JSON Schema language.
#[derive(Debug, Clone, Copy, Default)]
pub struct JsonSchema;

/// A JSON Schema the registry has read.
#[derive(Debug)]
pub struct ParsedJsonSchema {
    stored: String,
    arena: node::Arena,
}

impl SchemaFormat for JsonSchema {
    fn kind(&self) -> SchemaType {
        SchemaType::Json
    }

    fn parse(&self, text: &str, refs: &[ResolvedRef]) -> Result<Box<dyn ParsedSchema>, String> {
        parse(text, refs).map(|s| Box::new(s) as Box<dyn ParsedSchema>)
    }
}

/// Reads `text` with the schemas `refs` reach (dependencies first).
///
/// # Errors
/// The text or a reference is not JSON, not a valid schema of its draft, or uses a reference that reaches nothing.
// fuzz:parser json_schema
pub fn parse(text: &str, refs: &[ResolvedRef]) -> Result<ParsedJsonSchema, String> {
    let docs = doc::Docs::read(text, refs)?;
    check::check(&docs).map_err(|e| format!("Invalid JSON Schema: {e}"))?;
    let arena = node::load(&docs).map_err(|e| format!("Invalid JSON Schema: {e}"))?;
    let root = docs.doc(0).map(|d| &d.json).ok_or("no schema")?;
    Ok(ParsedJsonSchema {
        stored: stored_text(root),
        arena,
    })
}

/// The text Confluent stores: compact, an object's repeated key once (its last value, where the key first was, as
/// Jackson reads it), numbers as written.
fn stored_text(v: &Json) -> String {
    fn dedupe(v: &Json) -> Json {
        match v {
            Json::Object(_) => Json::Object(
                doc::members(v)
                    .into_iter()
                    .map(|(k, x)| (k.to_owned(), dedupe(x)))
                    .collect(),
            ),
            Json::Array(items) => Json::Array(items.iter().map(dedupe).collect()),
            other => other.clone(),
        }
    }
    dedupe(v).to_text()
}

impl ParsedSchema for ParsedJsonSchema {
    /// As Confluent 7.7 and the Go edition: `normalize` changes nothing for JSON Schema.
    fn canonical(&self, _normalize: bool) -> String {
        self.stored.clone()
    }

    fn can_read(
        &self,
        writer: &dyn ParsedSchema,
        reader_is: Role,
        writer_is: Role,
    ) -> Vec<CompatReason> {
        let Some(writer) = writer.as_any().downcast_ref::<Self>() else {
            return Vec::new();
        };
        compat::changes(&writer.arena, &self.arena)
            .into_iter()
            .map(|c| {
                let description =
                    compat::describe(c.kind, &c.path, reader_is.word(), writer_is.word());
                // The closing quote is Confluent's own: a double quote opens the description and a single one
                // closes it (recorded: json-reasons.json).
                let text = format!("{{errorType:\"{}\", description:\"{description}'}}", c.kind);
                CompatReason {
                    kind: c.kind.to_owned(),
                    path: c.path,
                    description,
                    info: String::new(),
                    text,
                }
            })
            .collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests;
