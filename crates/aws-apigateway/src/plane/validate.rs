//! Request validation of a REST method with a validator: required parameters (`requestParameters` marked `true`) must be present, and
//! the body must fit the method's model for its content type (JSON Schema draft 4: `type`, `properties`, `required`, `items`, `enum`,
//! `minimum`/`maximum`, `minLength`/`maxLength`, `pattern`, `minItems`/`maxItems`, `additionalProperties`, `$ref` to another model of
//! the API). The words are AWS's documented ones `(memory)`: `Missing required request parameters: [...]` and `Invalid request body`.

use aws_wire::service::WebReply;
use serde_json::{Map, Value};

use super::{Found, Incoming, Route, reply};
use crate::doc::text;

/// How deep a schema may be followed (`$ref` loops, deep documents).
const MAX_DEPTH: usize = 32;

pub(crate) fn rest(
    found: &Found,
    route: &Route,
    req: &Incoming,
    ctx: &Value,
) -> Result<(), WebReply> {
    let Some(v) = route.validator.as_ref() else {
        return Ok(());
    };
    if v.get("validateRequestParameters").and_then(Value::as_bool) == Some(true) {
        let missing = missing_params(route, req);
        if !missing.is_empty() {
            let msg = format!(
                "Missing required request parameters: [{}]",
                missing.join(", ")
            );
            return Err(reply::gateway(
                found,
                "BAD_REQUEST_PARAMETERS",
                400,
                &msg,
                req,
                Some(ctx),
            ));
        }
    }
    if v.get("validateRequestBody").and_then(Value::as_bool) == Some(true) {
        let ct = req
            .header("content-type")
            .and_then(|c| c.split(';').next())
            .map_or("application/json".to_owned(), |c| {
                c.trim().to_ascii_lowercase()
            });
        let models = crate::doc::obj(&route.method, "requestModels");
        let name = models
            .and_then(|m| m.get(&ct).or_else(|| m.get("$default")))
            .and_then(Value::as_str);
        if let Some(schema) = name
            .and_then(|n| found.models.get(n))
            .and_then(|m| text(m, "schema"))
        {
            let ok = serde_json::from_str::<Value>(schema).is_ok_and(|schema| {
                serde_json::from_slice::<Value>(&req.body)
                    .is_ok_and(|body| fits(&body, &schema, &found.models, 0))
            });
            if !ok {
                return Err(reply::gateway(
                    found,
                    "BAD_REQUEST_BODY",
                    400,
                    "Invalid request body",
                    req,
                    Some(ctx),
                ));
            }
        }
    }
    Ok(())
}

fn missing_params(route: &Route, req: &Incoming) -> Vec<String> {
    let mut out = Vec::new();
    for (k, required) in crate::doc::obj(&route.method, "requestParameters")
        .into_iter()
        .flatten()
    {
        if required.as_bool() != Some(true) {
            continue;
        }
        let present = if let Some(n) = k.strip_prefix("method.request.querystring.") {
            req.query_value(n).is_some()
        } else if let Some(n) = k.strip_prefix("method.request.header.") {
            req.header(&n.to_ascii_lowercase()).is_some()
        } else if let Some(n) = k.strip_prefix("method.request.path.") {
            route.params.iter().any(|(p, v)| p == n && !v.is_empty())
        } else {
            true
        };
        if !present {
            out.push(k.rsplit('.').next().unwrap_or(k).to_owned());
        }
    }
    out
}

/// Whether `v` fits `schema`.
pub(crate) fn fits(v: &Value, schema: &Value, models: &Map<String, Value>, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    if let Some(r) = text(schema, "$ref") {
        let name = r.rsplit('/').next().unwrap_or(r);
        let target = models
            .get(name)
            .and_then(|m| text(m, "schema"))
            .and_then(|s| serde_json::from_str::<Value>(s).ok());
        return target.is_some_and(|t| fits(v, &t, models, depth + 1));
    }
    if let Some(t) = schema.get("type")
        && !type_ok(v, t)
    {
        return false;
    }
    if let Some(e) = schema.get("enum").and_then(Value::as_array)
        && !e.contains(v)
    {
        return false;
    }
    match v {
        Value::Object(o) => object_ok(o, schema, models, depth),
        Value::Array(a) => array_ok(a, schema, models, depth),
        Value::String(s) => string_ok(s, schema),
        Value::Number(n) => number_ok(n.as_f64().unwrap_or(0.0), schema),
        _ => true,
    }
}

fn type_ok(v: &Value, t: &Value) -> bool {
    let one = |t: &str| match t {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "integer" => v.as_f64().is_some_and(|f| f.fract() == 0.0),
        "number" => v.is_number(),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        _ => true,
    };
    match t {
        Value::String(s) => one(s),
        Value::Array(list) => list.iter().filter_map(Value::as_str).any(one),
        _ => true,
    }
}

fn object_ok(
    o: &Map<String, Value>,
    schema: &Value,
    models: &Map<String, Value>,
    depth: usize,
) -> bool {
    let props = crate::doc::obj(schema, "properties");
    let required = schema.get("required").and_then(Value::as_array);
    if required
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|r| !o.contains_key(r))
    {
        return false;
    }
    let closed = schema.get("additionalProperties") == Some(&Value::Bool(false));
    o.iter().all(|(k, x)| match props.and_then(|p| p.get(k)) {
        Some(s) => fits(x, s, models, depth + 1),
        None => !closed,
    })
}

fn array_ok(a: &[Value], schema: &Value, models: &Map<String, Value>, depth: usize) -> bool {
    let n = a.len() as u64;
    let bound = |k: &str| schema.get(k).and_then(Value::as_u64);
    bound("minItems").is_none_or(|m| n >= m)
        && bound("maxItems").is_none_or(|m| n <= m)
        && schema
            .get("items")
            .is_none_or(|items| a.iter().all(|x| fits(x, items, models, depth + 1)))
}

fn string_ok(s: &str, schema: &Value) -> bool {
    let n = s.chars().count() as u64;
    let bound = |k: &str| schema.get(k).and_then(Value::as_u64);
    bound("minLength").is_none_or(|m| n >= m)
        && bound("maxLength").is_none_or(|m| n <= m)
        && text(schema, "pattern")
            .is_none_or(|p| regex_lite::Regex::new(p).is_ok_and(|r| r.is_match(s)))
}

fn number_ok(f: f64, schema: &Value) -> bool {
    let bound = |k: &str| schema.get(k).and_then(Value::as_f64);
    let exclusive = |k: &str| schema.get(k).and_then(Value::as_bool) == Some(true);
    bound("minimum").is_none_or(|m| {
        if exclusive("exclusiveMinimum") {
            f > m
        } else {
            f >= m
        }
    }) && bound("maximum").is_none_or(|m| {
        if exclusive("exclusiveMaximum") {
            f < m
        } else {
            f <= m
        }
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn bodies_are_held_to_their_schema() {
        let schema = json!({"type": "object", "required": ["name"], "additionalProperties": false,
            "properties": {"name": {"type": "string", "minLength": 2}, "n": {"type": "integer", "minimum": 1},
                "tags": {"type": "array", "items": {"enum": ["a", "b"]}}, "pet": {"$ref": "https://apigateway.amazonaws.com/restapis/x/models/Pet"}}});
        let models: Map<String, Value> = [(
            "Pet".to_owned(),
            json!({"schema": "{\"type\":\"object\",\"required\":[\"kind\"]}"}),
        )]
        .into_iter()
        .collect();
        let ok = |v: Value| fits(&v, &schema, &models, 0);
        assert!(ok(
            json!({"name": "pat", "n": 2, "tags": ["a"], "pet": {"kind": "cat"}})
        ));
        assert!(!ok(json!({"n": 2})));
        assert!(!ok(json!({"name": "p"})));
        assert!(!ok(json!({"name": "pat", "n": 1.5})));
        assert!(!ok(json!({"name": "pat", "tags": ["c"]})));
        assert!(!ok(json!({"name": "pat", "extra": 1})));
        assert!(!ok(json!({"name": "pat", "pet": {}})));
    }
}
