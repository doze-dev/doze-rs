//! Gateway responses: what API Gateway answers when it answers for itself (no route, unauthorized, a bad body, a failed integration).
//! Every type has a default; `PutGatewayResponse` replaces it for one API (REST APIs only). The types and their statuses are from the
//! model's enum and AWS's documentation; the default template is the one AWS documents (`(memory)` for the exact text).

use serde_json::{Map, Value, json};

/// Each type and its status (`None` for the two defaults, which take the status of what went wrong).
pub(crate) const TYPES: &[(&str, Option<u16>)] = &[
    ("ACCESS_DENIED", Some(403)),
    ("API_CONFIGURATION_ERROR", Some(500)),
    ("AUTHORIZER_CONFIGURATION_ERROR", Some(500)),
    ("AUTHORIZER_FAILURE", Some(500)),
    ("BAD_REQUEST_BODY", Some(400)),
    ("BAD_REQUEST_PARAMETERS", Some(400)),
    ("DEFAULT_4XX", None),
    ("DEFAULT_5XX", None),
    ("EXPIRED_TOKEN", Some(403)),
    ("INTEGRATION_FAILURE", Some(504)),
    ("INTEGRATION_TIMEOUT", Some(504)),
    ("INVALID_API_KEY", Some(403)),
    ("INVALID_SIGNATURE", Some(403)),
    ("MISSING_AUTHENTICATION_TOKEN", Some(403)),
    ("QUOTA_EXCEEDED", Some(429)),
    ("REQUEST_TOO_LARGE", Some(413)),
    ("RESOURCE_NOT_FOUND", Some(404)),
    ("THROTTLED", Some(429)),
    ("UNAUTHORIZED", Some(401)),
    ("UNSUPPORTED_MEDIA_TYPE", Some(415)),
    ("WAF_FILTERED", Some(403)),
];

const TEMPLATE: &str = "{\"message\":$context.error.messageString}";

/// The default response of `kind`.
pub(crate) fn default_of(kind: &str) -> Value {
    let mut v = Map::new();
    v.insert("defaultResponse".into(), json!(true));
    v.insert("responseParameters".into(), json!({}));
    v.insert(
        "responseTemplates".into(),
        json!({"application/json": TEMPLATE}),
    );
    v.insert("responseType".into(), json!(kind));
    if let Some(code) = TYPES.iter().find(|(t, _)| *t == kind).and_then(|(_, c)| *c) {
        v.insert("statusCode".into(), json!(code.to_string()));
    }
    Value::Object(v)
}

/// The response of `kind` an API has: its own, or the default.
pub(crate) fn of(own: Option<&Map<String, Value>>, kind: &str) -> Value {
    own.and_then(|m| m.get(kind))
        .cloned()
        .unwrap_or_else(|| default_of(kind))
}

/// Every type's response, the API's own where it set one.
pub(crate) fn all(own: Option<&Map<String, Value>>) -> Vec<Value> {
    TYPES.iter().map(|(t, _)| of(own, t)).collect()
}

/// The default type a status falls back to: `DEFAULT_4XX` or `DEFAULT_5XX`.
pub(crate) fn fallback(status: u16) -> &'static str {
    if status >= 500 {
        "DEFAULT_5XX"
    } else {
        "DEFAULT_4XX"
    }
}
