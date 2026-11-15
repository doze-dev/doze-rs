//! Avro, the registry's default language: parsed and checked as Confluent's Avro parser does (`schema`), stored as
//! Avro's own library writes a schema (`text`), compared by Avro's reader/writer rules with Confluent's sentences
//! (`compat`), and shown with its referenced types written out (`inline`).

mod compat;
mod inline;
mod schema;
mod text;

use std::any::Any;

use crate::format::{CompatReason, ParsedSchema, ResolvedRef, Role, SchemaFormat, SchemaType};
use crate::json::Json;

pub(crate) use inline::inline;

/// The Avro language.
#[derive(Debug, Clone, Copy, Default)]
pub struct Avro;

/// An Avro schema the registry has read.
#[derive(Debug)]
pub struct AvroSchema {
    stored: String,
    normalized: String,
    schema: schema::Schema,
}

impl SchemaFormat for Avro {
    fn kind(&self) -> SchemaType {
        SchemaType::Avro
    }

    fn parse(&self, text: &str, refs: &[ResolvedRef]) -> Result<Box<dyn ParsedSchema>, String> {
        parse(text, refs).map(|s| Box::new(s) as Box<dyn ParsedSchema>)
    }
}

/// Reads `text` with the named types `refs` define (dependencies first).
///
/// # Errors
/// Either text is not an Avro schema.
// fuzz:parser avro_schema
pub fn parse(text: &str, refs: &[ResolvedRef]) -> Result<AvroSchema, String> {
    let mut parser = schema::Parser::default();
    let mut known = Vec::new();
    let mut read: Vec<(&str, u32)> = Vec::new();
    for r in refs {
        // One schema reached by two names is read once: its types are defined once.
        if read.contains(&(r.subject.as_str(), r.version)) {
            continue;
        }
        read.push((r.subject.as_str(), r.version));
        let j = Json::parse(&r.text).map_err(|e| format!("reference \"{}\": {e}", r.name))?;
        parser
            .read(&j)
            .map_err(|e| format!("reference \"{}\": {e}", r.name))?;
        known.extend(text::defined_names(&j));
    }
    let j = Json::parse(text).map_err(|e| e.to_string())?;
    let root = parser.read(&j)?;
    Ok(AvroSchema {
        stored: text::stored_text(&j, &known, false)?,
        normalized: text::stored_text(&j, &known, true)?,
        schema: parser.finish(root),
    })
}

impl ParsedSchema for AvroSchema {
    fn canonical(&self, normalize: bool) -> String {
        if normalize {
            self.normalized.clone()
        } else {
            self.stored.clone()
        }
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
        compat::incompatibilities(&self.schema, &writer.schema)
            .iter()
            .map(|f| compat::reason(f, reader_is, writer_is))
            .collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests;
