//! Go's one suggested fix for an incompatible draft: a field of a new Avro schema that old data lacks, given its type's
//! zero as a default. Made here on the draft and sent back whole; nothing is registered by it.

use kafka_registry::format::{CompatReason, SchemaType};
use kafka_registry::json::Json;

use super::write::{FixView, SchemaPost};

/// The zero value Avro would take as a default for a field of this type, if it has one Go offered.
fn zero_of(t: &Json) -> Option<Json> {
    let raw = |s: &str| Some(Json::Raw(s.to_owned()));
    match t {
        Json::Str(s) => match s.as_str() {
            "null" => raw("null"),
            "string" | "bytes" => Some(Json::Str(String::new())),
            "int" | "long" | "float" | "double" => raw("0"),
            "boolean" => raw("false"),
            _ => None,
        },
        Json::Array(branches) => branches.first().and_then(zero_of),
        Json::Object(_) => match t.get("type").and_then(Json::as_str) {
            Some("array") => Some(Json::Array(Vec::new())),
            Some("map") => Some(Json::Object(Vec::new())),
            Some(prim) => zero_of(&Json::Str(prim.to_owned())),
            None => None,
        },
        Json::Raw(_) => None,
    }
}

/// The value a JSON pointer names, to change.
fn at_pointer<'a>(j: &'a mut Json, pointer: &str) -> Option<&'a mut Json> {
    let mut here = j;
    for token in pointer.split('/').skip(1).filter(|t| !t.is_empty()) {
        let token = token.replace("~1", "/").replace("~0", "~");
        here = match here {
            Json::Object(m) => m.iter_mut().find(|(k, _)| *k == token).map(|(_, v)| v)?,
            Json::Array(items) => items.get_mut(token.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(here)
}

/// Go's one fix: a field of the new Avro schema that old data lacks, given its type's zero as a default.
pub(super) fn fix(p: &SchemaPost, r: &CompatReason) -> Option<FixView> {
    if r.kind != "READER_FIELD_MISSING_DEFAULT_VALUE"
        || SchemaType::named(&p.schema_type) != Some(SchemaType::Avro)
        || !r.description.contains("in the new schema")
    {
        return None;
    }
    let mut j = Json::parse(&p.schema).ok()?;
    let field = at_pointer(&mut j, &r.path)?;
    let zero = zero_of(field.get("type")?)?;
    let name = field
        .get("name")
        .and_then(Json::as_str)
        .unwrap_or_default()
        .to_owned();
    let label = format!("Add \"default\": {} to {name}", zero.to_text());
    field.set("default", zero);
    Some(FixView {
        label,
        schema: kafka_registry::codec::pretty(&j),
    })
}
