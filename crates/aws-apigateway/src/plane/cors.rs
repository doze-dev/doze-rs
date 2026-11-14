//! An HTTP API's `corsConfiguration`, applied on the plane: a preflight (`OPTIONS` with `Origin` and `Access-Control-Request-Method`)
//! is answered by API Gateway itself, 204, with the allow headers when the origin is allowed and without them when it is not; every
//! other answer to an allowed origin carries `Access-Control-Allow-Origin` and the expose and credentials headers.

use aws_wire::service::WebReply;
use bytes::Bytes;
use serde_json::Value;

use super::Incoming;
use super::mapping::plain;

fn list(cors: &Value, key: &str) -> Vec<String> {
    cors.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().map(plain).collect())
        .unwrap_or_default()
}

/// The origin to name in `Access-Control-Allow-Origin`, when the request's is allowed.
fn allowed_origin(cors: &Value, req: &Incoming) -> Option<String> {
    let origin = req.header("origin")?;
    let allowed = list(cors, "allowOrigins");
    let credentials = cors.get("allowCredentials").and_then(Value::as_bool) == Some(true);
    if allowed.iter().any(|o| o == origin) {
        return Some(origin.to_owned());
    }
    allowed.iter().any(|o| o == "*").then(|| {
        if credentials {
            origin.to_owned()
        } else {
            "*".to_owned()
        }
    })
}

/// The answer to a preflight, when the request is one and the API has a CORS configuration.
pub(crate) fn preflight(cors: Option<&Value>, req: &Incoming) -> Option<WebReply> {
    let cors = cors?;
    if req.method != "OPTIONS"
        || req.header("origin").is_none()
        || req.header("access-control-request-method").is_none()
    {
        return None;
    }
    let mut headers = Vec::new();
    if let Some(origin) = allowed_origin(cors, req) {
        headers.push(("Access-Control-Allow-Origin".to_owned(), origin));
        let methods = list(cors, "allowMethods");
        if !methods.is_empty() {
            headers.push(("Access-Control-Allow-Methods".into(), methods.join(",")));
        }
        let allow_headers = list(cors, "allowHeaders");
        if !allow_headers.is_empty() {
            headers.push((
                "Access-Control-Allow-Headers".into(),
                allow_headers.join(","),
            ));
        }
        if let Some(age) = cors.get("maxAge").and_then(Value::as_i64) {
            headers.push(("Access-Control-Max-Age".into(), age.to_string()));
        }
        if cors.get("allowCredentials").and_then(Value::as_bool) == Some(true) {
            headers.push(("Access-Control-Allow-Credentials".into(), "true".into()));
        }
    }
    Some(WebReply {
        status: 204,
        headers,
        body: Bytes::new(),
    })
}

/// Adds the CORS headers of an allowed origin to an answer.
pub(crate) fn decorate(cors: Option<&Value>, req: &Incoming, reply: &mut WebReply) {
    let Some(cors) = cors else { return };
    let Some(origin) = allowed_origin(cors, req) else {
        return;
    };
    reply
        .headers
        .retain(|(k, _)| !k.eq_ignore_ascii_case("access-control-allow-origin"));
    reply
        .headers
        .push(("Access-Control-Allow-Origin".into(), origin));
    let expose = list(cors, "exposeHeaders");
    if !expose.is_empty() {
        reply
            .headers
            .push(("Access-Control-Expose-Headers".into(), expose.join(",")));
    }
    if cors.get("allowCredentials").and_then(Value::as_bool) == Some(true) {
        reply
            .headers
            .push(("Access-Control-Allow-Credentials".into(), "true".into()));
    }
}
