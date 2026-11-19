//! Request bodies read as Confluent's registry reads them (Jackson, bound to its request classes): the first JSON value,
//! a key written twice keeping its last value, a scalar where a string is wanted taken as its text, nothing but white
//! space after the value, and Confluent's sentences for each refusal (recorded by `tools/registry-diff`).

use serde_json::Value;

use crate::error::RegistryError;
use crate::registry::SchemaInput;
use crate::store::Reference;

/// What a body is bound to: the Java class Confluent reads it into, and the argument its validator names when the body
/// is missing or `null`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bound {
    pub(crate) class: &'static str,
    pub(crate) arg: &'static str,
}

pub(crate) const REGISTER: Bound = Bound {
    class: "RegisterSchemaRequest",
    arg: "register.arg4",
};
pub(crate) const LOOKUP: Bound = Bound {
    class: "RegisterSchemaRequest",
    arg: "lookUpSchemaUnderSubject.arg4",
};
pub(crate) const COMPAT_VERSION: Bound = Bound {
    class: "RegisterSchemaRequest",
    arg: "testCompatibilityBySubjectName.arg4",
};
pub(crate) const COMPAT_LEVEL: Bound = Bound {
    class: "RegisterSchemaRequest",
    arg: "testCompatibilityForSubject.arg3",
};
pub(crate) const CONFIG: Bound = Bound {
    class: "ConfigUpdateRequest",
    arg: "updateTopLevelConfig.arg1",
};
pub(crate) const SUBJECT_CONFIG: Bound = Bound {
    class: "ConfigUpdateRequest",
    arg: "updateSubjectLevelConfig.arg2",
};
pub(crate) const MODE: Bound = Bound {
    class: "ModeUpdateRequest",
    arg: "updateTopLevelMode.arg1",
};
pub(crate) const SUBJECT_MODE: Bound = Bound {
    class: "ModeUpdateRequest",
    arg: "updateMode.arg2",
};

fn bad(message: String) -> RegistryError {
    RegistryError::http(400, message)
}

/// Jackson's refusal of a word that is not JSON.
fn unrecognized(word: &str) -> RegistryError {
    bad(format!(
        "Unrecognized token '{word}': was expecting (JSON String, Number, Array, Object or token 'null', 'true' or 'false')"
    ))
}

/// The word at the start of `rest`, as Jackson quotes an unrecognized token: up to white space or a structural character.
fn word_at(rest: &str) -> &str {
    let end = rest
        .find(|c: char| c.is_whitespace() || matches!(c, ',' | ':' | '{' | '}' | '[' | ']' | '"'))
        .unwrap_or(rest.len());
    rest.get(..end).unwrap_or_default()
}

/// Jackson's name for the token that starts `rest`, or `None` when it is no token (an unrecognized word).
fn token_kind(rest: &str) -> Option<&'static str> {
    let c = rest.chars().next()?;
    Some(match c {
        '{' => "START_OBJECT",
        '[' => "START_ARRAY",
        '}' => "END_OBJECT",
        ']' => "END_ARRAY",
        '"' => "VALUE_STRING",
        '-' | '0'..='9' => {
            let w = word_at(rest);
            if w.contains(['.', 'e', 'E']) {
                "VALUE_NUMBER_FLOAT"
            } else {
                "VALUE_NUMBER_INT"
            }
        }
        _ => match word_at(rest) {
            "true" => "VALUE_TRUE",
            "false" => "VALUE_FALSE",
            "null" => "VALUE_NULL",
            _ => return None,
        },
    })
}

/// The body's one JSON value (`None` for no body or `null`), as Jackson reads it for `bound`.
///
/// # Errors
/// Confluent's 400 for text that is not JSON or has more after the value.
fn value(raw: &[u8], bound: Bound) -> Result<Option<Value>, RegistryError> {
    let text = String::from_utf8_lossy(raw);
    let start = text.trim_start();
    if start.is_empty() {
        return Ok(None);
    }
    let mut stream = serde_json::Deserializer::from_str(start).into_iter::<Value>();
    let v = match stream.next() {
        Some(Ok(v)) => v,
        Some(Err(e)) => {
            // A word that is no JSON token is Jackson's "Unrecognized token"; other syntax errors are said in serde's words.
            let at = start.get(..).unwrap_or_default();
            if token_kind(at).is_none() {
                return Err(unrecognized(word_at(at)));
            }
            return Err(bad(format!("Unrecognized token in the request body: {e}")));
        }
        None => return Ok(None),
    };
    let rest = start
        .get(stream.byte_offset()..)
        .unwrap_or_default()
        .trim_start();
    if !rest.is_empty() {
        return Err(match token_kind(rest) {
            None => unrecognized(word_at(rest)),
            Some(kind) => bad(format!(
                "Trailing token (of type {kind}) found after value (bound as `{}`): not allowed as per `DeserializationFeature.FAIL_ON_TRAILING_TOKENS`",
                bound.class
            )),
        });
    }
    Ok((!v.is_null()).then_some(v))
}

/// The object a body must be, or Jackson's refusal of what it is instead.
fn object(v: Value, bound: Bound) -> Result<serde_json::Map<String, Value>, RegistryError> {
    let class = bound.class;
    match v {
        Value::Object(m) => Ok(m),
        Value::Array(_) => Err(bad(format!(
            "Cannot deserialize value of type `{class}` from Array value (token `JsonToken.START_ARRAY`)"
        ))),
        Value::String(s) => Err(bad(format!(
            "Cannot construct instance of `{class}` (although at least one Creator exists): no String-argument constructor/factory method to deserialize from String value ('{s}')"
        ))),
        Value::Number(n) => Err(bad(format!(
            "Cannot construct instance of `{class}` (although at least one Creator exists): no int/Int-argument constructor/factory method to deserialize from Number value ({n})"
        ))),
        Value::Bool(b) => Err(bad(format!(
            "Cannot construct instance of `{class}` (although at least one Creator exists): no boolean/Boolean-argument constructor/factory method to deserialize from boolean value ({b})"
        ))),
        Value::Null => Err(missing(bound)),
    }
}

fn missing(bound: Bound) -> RegistryError {
    RegistryError::http(422, format!("{} must not be null (was null)", bound.arg))
}

/// A string field: a scalar is taken as its text, as Jackson coerces it.
fn string(v: Option<&Value>, java: &str) -> Result<Option<String>, RegistryError> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(Value::Bool(b)) => Ok(Some(b.to_string())),
        Some(Value::Object(_)) => Err(bad(format!(
            "Cannot deserialize value of type `{java}` from Object value (token `JsonToken.START_OBJECT`)"
        ))),
        Some(Value::Array(_)) => Err(bad(format!(
            "Cannot deserialize value of type `{java}` from Array value (token `JsonToken.START_ARRAY`)"
        ))),
    }
}

/// A whole-number field: a number, or a string of one, as Jackson coerces it.
fn integer(v: Option<&Value>, field: &str) -> Result<Option<i64>, RegistryError> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_i64()
            .map(Some)
            .ok_or_else(|| bad(format!("Numeric value ({n}) out of range of int (field `{field}`)"))),
        Some(Value::String(s)) => s.trim().parse().map(Some).map_err(|_| {
            bad(format!(
                "Cannot deserialize value of type `int` from String \"{s}\": not a valid `int` value"
            ))
        }),
        Some(other) => Err(bad(format!(
            "Cannot deserialize value of type `int` from {} (field `{field}`)",
            if other.is_object() { "Object value" } else { "Array value" }
        ))),
    }
}

/// A true-or-false field: a boolean, or the string `true` or `false`, as Jackson coerces it.
fn boolean(v: Option<&Value>) -> Result<Option<bool>, RegistryError> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(Value::String(s)) if s == "true" || s == "false" => Ok(Some(s == "true")),
        Some(Value::String(s)) => Err(bad(format!(
            "Cannot deserialize value of type `Boolean` from String \"{s}\": only \"true\" or \"false\" recognized"
        ))),
        Some(Value::Number(n)) => Ok(Some(n.as_f64().is_some_and(|f| f != 0.0))),
        Some(_) => Err(bad("Cannot deserialize value of type `Boolean` from Object value (token `JsonToken.START_OBJECT`)".to_owned())),
    }
}

/// A schema request's body: `schema`, `schemaType`, `references`, and for IMPORT `id` and `version`. Anything else in it
/// (`metadata`, `ruleSet`, fields of later releases) is read past.
///
/// # Errors
/// Confluent's 400 for a body that is not a request, its 422 for none.
pub(crate) fn schema_request(raw: &[u8], bound: Bound) -> Result<SchemaInput, RegistryError> {
    let v = value(raw, bound)?.ok_or_else(|| missing(bound))?;
    let m = object(v, bound)?;
    let references = match m.get("references") {
        None | Some(Value::Null) => None,
        Some(Value::Array(items)) => {
            Some(items.iter().map(reference).collect::<Result<Vec<_>, _>>()?)
        }
        Some(Value::Object(_)) => {
            return Err(bad(
                "Cannot deserialize value of type `SchemaReference>` from Object value (token `JsonToken.START_OBJECT`)"
                    .to_owned(),
            ));
        }
        Some(_) => {
            return Err(bad(
                "Cannot deserialize value of type `SchemaReference>` from a scalar value"
                    .to_owned(),
            ));
        }
    };
    Ok(SchemaInput {
        schema: string(m.get("schema"), "String")?,
        schema_type: string(m.get("schemaType"), "String")?,
        references,
        id: integer(m.get("id"), "id")?,
        version: integer(m.get("version"), "version")?,
    })
}

/// A reference as a request gives it. One without a subject or a version is kept with them empty (`""`, `i64::MIN`),
/// for the registry to refuse in Confluent's words (`Invalid reference: {name='x', subject='null', version=null}`).
fn reference(v: &Value) -> Result<Reference, RegistryError> {
    let Value::Object(m) = v else {
        return Err(bad(
            "Cannot construct instance of `SchemaReference`: a reference is an object".to_owned(),
        ));
    };
    Ok(Reference {
        name: string(m.get("name"), "String")?.unwrap_or_default(),
        subject: string(m.get("subject"), "String")?.unwrap_or_default(),
        version: integer(m.get("version"), "version")?.unwrap_or(Reference::NO_VERSION),
    })
}

/// A level request's body (`ConfigUpdateRequest`): the level and the other settings Confluent keeps beside it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ConfigBody {
    pub(crate) compatibility: Option<String>,
    pub(crate) extra: crate::registry::ConfigExtra,
}

/// # Errors
/// Confluent's 400 for a body that is not a request, its 422 for none.
pub(crate) fn config_request(raw: &[u8], bound: Bound) -> Result<ConfigBody, RegistryError> {
    let v = value(raw, bound)?.ok_or_else(|| missing(bound))?;
    let m = object(v, bound)?;
    let json = |k: &str| {
        m.get(k)
            .filter(|v| !v.is_null())
            .map(|v| serde_json::to_string(v).unwrap_or_default())
    };
    Ok(ConfigBody {
        compatibility: string(m.get("compatibility"), "String")?,
        extra: crate::registry::ConfigExtra {
            alias: string(m.get("alias"), "String")?,
            normalize: boolean(m.get("normalize"))?,
            validate_fields: boolean(m.get("validateFields"))?,
            compatibility_group: string(m.get("compatibilityGroup"), "String")?,
            default_metadata: json("defaultMetadata"),
            override_metadata: json("overrideMetadata"),
            default_rule_set: json("defaultRuleSet"),
            override_rule_set: json("overrideRuleSet"),
        },
    })
}

/// A mode request's body (`ModeUpdateRequest`): the mode, `None` when it has none.
///
/// # Errors
/// Confluent's 400 for a body that is not a request, its 422 for none.
pub(crate) fn mode_request(raw: &[u8], bound: Bound) -> Result<Option<String>, RegistryError> {
    let v = value(raw, bound)?.ok_or_else(|| missing(bound))?;
    let m = object(v, bound)?;
    string(m.get("mode"), "String")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(raw: &str, bound: Bound) -> (u16, String) {
        let e = schema_request(raw.as_bytes(), bound).unwrap_err();
        (e.status, e.message)
    }

    // Every sentence here is cp-schema-registry 7.7.1's, recorded by tools/registry-diff.
    #[test]
    fn bodies_are_read_as_jackson_reads_them() {
        assert_eq!(
            refused("", REGISTER),
            (422, "register.arg4 must not be null (was null)".into())
        );
        assert_eq!(
            refused(" null ", COMPAT_LEVEL),
            (
                422,
                "testCompatibilityForSubject.arg3 must not be null (was null)".into()
            )
        );
        assert_eq!(
            refused("[]", LOOKUP),
            (400, "Cannot deserialize value of type `RegisterSchemaRequest` from Array value (token `JsonToken.START_ARRAY`)".into())
        );
        assert_eq!(
            refused("\"x\"", REGISTER).1,
            "Cannot construct instance of `RegisterSchemaRequest` (although at least one Creator exists): no String-argument constructor/factory method to deserialize from String value ('x')"
        );
        assert_eq!(
            refused("not json", REGISTER).1,
            "Unrecognized token 'not': was expecting (JSON String, Number, Array, Object or token 'null', 'true' or 'false')"
        );
        assert_eq!(
            refused(r#"{"schema":"\"int\""} {}"#, REGISTER).1,
            "Trailing token (of type START_OBJECT) found after value (bound as `RegisterSchemaRequest`): not allowed as per `DeserializationFeature.FAIL_ON_TRAILING_TOKENS`"
        );
        assert_eq!(
            refused(r#"{"schema":{}}"#, REGISTER).1,
            "Cannot deserialize value of type `String` from Object value (token `JsonToken.START_OBJECT`)"
        );
        // A scalar is a string's text; the last of a key written twice is the one read.
        let got = schema_request(br#"{"schema":5,"schema":true,"version":"3"}"#, REGISTER).unwrap();
        assert_eq!(
            (got.schema.as_deref(), got.version),
            (Some("true"), Some(3))
        );
        let got = schema_request(
            br#"{"schema":"\"int\"","references":[{"name":"x"}]}"#,
            REGISTER,
        )
        .unwrap();
        let r = &got.references.unwrap()[0];
        assert_eq!((r.subject.as_str(), r.version), ("", Reference::NO_VERSION));
    }

    #[test]
    fn settings_bodies_are_read_as_jackson_reads_them() {
        let e = config_request(br#"{"compatibility":"NONE"} 5"#, CONFIG).unwrap_err();
        assert!(
            e.message.contains("(of type VALUE_NUMBER_INT)")
                && e.message.contains("`ConfigUpdateRequest`")
        );
        let e = config_request(br#"{"compatibility":"NONE"} x"#, CONFIG).unwrap_err();
        assert!(
            e.message.starts_with("Unrecognized token 'x'"),
            "{}",
            e.message
        );
        let e = config_request(b"", SUBJECT_CONFIG).unwrap_err();
        assert_eq!(
            e.message,
            "updateSubjectLevelConfig.arg2 must not be null (was null)"
        );
        let c = config_request(
            br#"{"compatibility":5,"normalize":"true","compatibility":"FULL"}"#,
            CONFIG,
        )
        .unwrap();
        assert_eq!(
            (c.compatibility.as_deref(), c.extra.normalize),
            (Some("FULL"), Some(true))
        );
        let e = config_request(br#"{"normalize":"yes"}"#, CONFIG).unwrap_err();
        assert_eq!(
            e.message,
            "Cannot deserialize value of type `Boolean` from String \"yes\": only \"true\" or \"false\" recognized"
        );
        assert_eq!(
            mode_request(br#"{"mode":5}"#, MODE).unwrap().as_deref(),
            Some("5")
        );
        assert_eq!(
            mode_request(b"null", SUBJECT_MODE).unwrap_err().message,
            "updateMode.arg2 must not be null (was null)"
        );
    }
}
