//! `GetModelTemplate` (both APIs): a mapping template that writes a payload in the shape of a model, generated from the model's JSON
//! schema. Recorded 2026-11-19: AWS writes sample values, not references to the input: a string is `"foo"`, an integer `42`, a
//! boolean `true`, each property on its own line two spaces in per level; an array is a `#foreach` over `$inputRoot.TODO` behind
//! a `##TODO` comment, its element written once between single spaces, its `]` at the start of the line; the empty model is
//! `{ }`. A `number` written as `42` too, and a nested object written in place, are `(memory)`: the recording has neither. A
//! `$ref` to another model is followed (once per path, so a recursive model ends).

use serde_json::{Map, Value};

const ROOT: &str = "#set($inputRoot = $input.path('$'))";
/// How deep a template is written; deeper parts are written as a string sample.
const MAX_DEPTH: usize = 16;

/// The models a schema may `$ref`, by name.
pub(crate) type Models<'a> = &'a dyn Fn(&str) -> Option<Value>;

/// The template for a model's schema (its text).
pub(crate) fn generate(schema: &str, models: Models<'_>) -> String {
    let schema: Value = serde_json::from_str(schema).unwrap_or(Value::Null);
    let mut out = String::from(ROOT);
    out.push('\n');
    let mut seen = Vec::new();
    let shape = resolve(&schema, models, &mut seen);
    match kind(&shape) {
        "object" if props(&shape).is_none_or(Map::is_empty) => out.push_str("{ }"),
        _ => value(&mut out, &shape, 0, models, &mut seen),
    }
    out
}

/// The name a `$ref` names: `#/definitions/Pet` or the model URL `https://apigateway.amazonaws.com/restapis/<id>/models/Pet`.
fn ref_name(r: &str) -> &str {
    r.rsplit('/').next().unwrap_or(r)
}

fn resolve(schema: &Value, models: Models<'_>, seen: &mut Vec<String>) -> Value {
    match schema.get("$ref").and_then(Value::as_str) {
        Some(r) if !seen.iter().any(|s| s == ref_name(r)) => {
            let name = ref_name(r).to_owned();
            let target = models(&name).unwrap_or(Value::Null);
            seen.push(name);
            resolve(&target, models, seen)
        }
        Some(_) => Value::Null,
        None => schema.clone(),
    }
}

fn kind(schema: &Value) -> &str {
    match schema.get("type") {
        Some(Value::String(t)) => t,
        Some(Value::Array(ts)) => ts
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null")
            .unwrap_or("string"),
        _ if schema.get("properties").is_some() => "object",
        _ if schema.get("items").is_some() => "array",
        _ => "string",
    }
}

fn props(schema: &Value) -> Option<&Map<String, Value>> {
    schema.get("properties").and_then(Value::as_object)
}

/// The sample AWS writes for a scalar.
fn scalar(schema: &Value) -> &'static str {
    match kind(schema) {
        "integer" | "number" => "42",
        "boolean" => "true",
        _ => "\"foo\"",
    }
}

fn indent(out: &mut String, depth: usize) {
    out.push_str(&"  ".repeat(depth));
}

fn object(
    out: &mut String,
    schema: &Value,
    depth: usize,
    models: Models<'_>,
    seen: &mut Vec<String>,
) {
    out.push_str("{\n");
    let fields: Vec<(&String, &Value)> = props(schema).into_iter().flatten().collect();
    let last = fields.len().saturating_sub(1);
    for (i, (name, sub)) in fields.into_iter().enumerate() {
        indent(out, depth + 1);
        out.push_str(&format!("\"{name}\" : "));
        let mark = seen.len();
        let sub = resolve(sub, models, seen);
        value(out, &sub, depth + 1, models, seen);
        seen.truncate(mark);
        if i < last {
            out.push(',');
        }
        out.push('\n');
    }
    indent(out, depth);
    out.push('}');
}

fn value(
    out: &mut String,
    schema: &Value,
    depth: usize,
    models: Models<'_>,
    seen: &mut Vec<String>,
) {
    if depth >= MAX_DEPTH {
        out.push_str("\"foo\"");
        return;
    }
    match kind(schema) {
        "object" => object(out, schema, depth, models, seen),
        "array" => array(out, schema, depth, models, seen),
        _ => out.push_str(scalar(schema)),
    }
}

fn array(
    out: &mut String,
    schema: &Value,
    depth: usize,
    models: Models<'_>,
    seen: &mut Vec<String>,
) {
    out.push_str("[\n");
    out.push_str("##TODO: Update this foreach loop to reference array from input json\n");
    out.push_str("#foreach($elem in $inputRoot.TODO)\n ");
    let items = schema.get("items").cloned().unwrap_or(Value::Null);
    let mark = seen.len();
    let items = resolve(&items, models, seen);
    value(out, &items, depth + 1, models, seen);
    seen.truncate(mark);
    out.push_str(" \n");
    out.push_str("#if($foreach.hasNext),#end\n");
    out.push_str("#end\n");
    out.push(']');
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use serde_json::json;

    use super::*;

    #[test]
    fn the_empty_model_is_an_empty_object() {
        let none = |_: &str| None;
        assert_eq!(
            generate(
                r#"{"$schema": "http://json-schema.org/draft-04/schema#", "title": "Empty Schema", "type": "object"}"#,
                &none
            ),
            "#set($inputRoot = $input.path('$'))\n{ }"
        );
    }

    #[test]
    fn samples_are_written_as_aws_writes_them() {
        // Recorded 2026-11-19.
        let none = |_: &str| None;
        let t = generate(
            r#"{"type": "object", "properties": {"name": {"type": "string"}, "age": {"type": "integer"},
                "tags": {"type": "array", "items": {"type": "string"}}}}"#,
            &none,
        );
        assert_eq!(
            t,
            "#set($inputRoot = $input.path('$'))\n{\n  \"name\" : \"foo\",\n  \"age\" : 42,\n  \"tags\" : [\n##TODO: Update this foreach loop to reference array from input json\n#foreach($elem in $inputRoot.TODO)\n \"foo\" \n#if($foreach.hasNext),#end\n#end\n]\n}"
        );
        let ok = generate(
            r#"{"type": "object", "properties": {"ok": {"type": "boolean"}}}"#,
            &none,
        );
        assert_eq!(
            ok,
            "#set($inputRoot = $input.path('$'))\n{\n  \"ok\" : true\n}"
        );
    }

    #[test]
    fn references_are_followed_and_the_template_runs() {
        let pet = json!({"type": "object", "properties": {"id": {"type": "integer"}}});
        let models = move |n: &str| (n == "Pet").then(|| pet.clone());
        let t = generate(
            r##"{"type": "object", "properties": {"pets": {"type": "array", "items": {"$ref": "#/definitions/Pet"}}}}"##,
            &models,
        );
        assert!(t.contains("\"id\" : 42"), "{t}");
        // The template runs on the plane's VTL.
        let mut env = crate::plane::vtl::Env::default();
        assert!(crate::plane::vtl::render(&t, &mut env).is_ok());
    }

    #[test]
    fn a_recursive_model_ends() {
        let node =
            json!({"type": "object", "properties": {"next": {"$ref": "#/definitions/Node"}}});
        let models = move |n: &str| (n == "Node").then(|| node.clone());
        let t = generate(r##"{"$ref": "#/definitions/Node"}"##, &models);
        assert!(t.contains("\"next\" : "), "{t}");
    }
}
