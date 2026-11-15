//! JSON Web Tokens: an HTTP API's `JWT` authorizer and a REST API's `COGNITO_USER_POOLS` authorizer.
//!
//! **JWT** (HTTP APIs), in AWS's order: the token is read; it must be signed with an RSA algorithm ("Currently, only RSA-based
//! algorithms are supported": RS256, RS384, RS512; an ES256 or HS256 token is refused); the signature is verified against the issuer's
//! JWKS (`aws_webhook::jwks`: fetched over `https://` or `http://`, kept two hours); then `iss`, `exp`, `nbf`, the audience (`aud`, else
//! `client_id`) and the route's scopes (`scope`/`scp`). A refusal is 401 `{"message":"Unauthorized"}` with a
//! `www-authenticate: Bearer scope="" error="invalid_token" error_description="…"` header, a missing scope 403 `Forbidden` with
//! `error="insufficient_scope"` (the header and its descriptions are (memory)). When the keys cannot be fetched (the issuer is down,
//! or its certificate is refused), the token is refused, as AWS refuses it, and doze-aws's log says why (with the flag that fixes a
//! refused certificate).
//!
//! **COGNITO_USER_POOLS** (REST APIs): the claims are checked; the signature is not, because Cognito is not served here and a token
//! from a local test cannot be signed by a real pool.

use std::time::Duration;

use aws_webhook::jwks::Jwks;
use aws_webhook::jwt::{Refusal, Token, decode};
use aws_wire::service::WebReply;
use serde_json::{Value, json};

use super::event::Context;
use super::mapping::{plain, v2_source};
use super::{Found, Incoming, Route, reply};
use crate::doc::text;

/// "Timeout for JSON Web Key Set endpoint: 1500 ms".
#[cfg(not(test))]
const FETCH_TIMEOUT: Duration = Duration::from_millis(1500);
/// The unit tests' debug-built TLS handshakes, under a whole workspace's test load, can take longer than AWS's 1,500 ms.
#[cfg(test)]
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// "Response size from JSON Web Key Set endpoint: 150000 bytes".
const MAX_ANSWER: usize = 150_000;

/// The issuers' keys, fetched within the HTTP API quotas above.
pub(crate) fn keys() -> Jwks {
    Jwks::new(FETCH_TIMEOUT, MAX_ANSWER)
}

/// Whether the signature verifies under one of the issuer's keys ("Currently, only RSA-based algorithms are supported").
async fn signature_ok(jwks: &Jwks, issuer: &str, t: &Token, now_ms: i64) -> Result<(), String> {
    t.verify(jwks, issuer, aws_webhook::jwt::RSA, now_ms)
        .await
        .map_err(|r| {
            if let Refusal::Unfetchable(why) = &r {
                tracing::warn!("JWT authorizer: the keys of {issuer} could not be fetched, so the token is refused: {why}");
            }
            r.to_string()
        })
}

/// The JWT authorizer's refusal: 401, and why in `www-authenticate` (memory).
fn refused(why: &str) -> WebReply {
    tracing::debug!("JWT authorizer: {why}");
    let mut r = reply::message(401, "Unauthorized");
    r.headers.push((
        "www-authenticate".into(),
        format!(
            "Bearer scope=\"\" error=\"invalid_token\" error_description=\"{}\"",
            why.replace('"', "'")
        ),
    ));
    r
}

/// Why the claims refuse the token, if they do.
fn claims_refuse(claims: &Value, config: &Value, now_secs: i64) -> Option<String> {
    let at = |k: &str| claims.get(k).and_then(Value::as_i64);
    if at("exp").is_some_and(|e| e <= now_secs) {
        return Some("the token has expired".into());
    }
    if at("nbf").is_some_and(|n| n > now_secs) {
        return Some("the token is not valid yet".into());
    }
    let audience: Vec<String> = config
        .get("audience")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(plain).collect())
        .unwrap_or_default();
    let aud: Vec<String> = match claims.get("aud") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().map(plain).collect(),
        _ => text(claims, "client_id")
            .map(|c| vec![c.to_owned()])
            .unwrap_or_default(),
    };
    (!audience.is_empty() && !aud.iter().any(|a| audience.contains(a)))
        .then(|| "the token does not have a valid audience".into())
}

fn times_ok(claims: &Value, now_secs: i64) -> bool {
    let at = |k: &str| claims.get(k).and_then(Value::as_i64);
    at("exp").is_none_or(|e| e > now_secs) && at("nbf").is_none_or(|n| n <= now_secs)
}

fn scopes_of(claims: &Value) -> Vec<String> {
    match claims.get("scope").or_else(|| claims.get("scp")) {
        Some(Value::String(s)) => s.split_whitespace().map(str::to_owned).collect(),
        Some(Value::Array(a)) => a.iter().map(plain).collect(),
        _ => Vec::new(),
    }
}

fn wanted_scopes(route: &Route) -> Vec<String> {
    route
        .method
        .get("authorizationScopes")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(plain).collect())
        .unwrap_or_default()
}

/// An HTTP API's `JWT` authorizer.
pub(crate) async fn v2(
    jwks: &Jwks,
    found: &Found,
    route: &Route,
    req: &Incoming,
    extra: &mut Context,
) -> Result<(), WebReply> {
    let auth = route
        .authorizer
        .as_ref()
        .ok_or_else(|| reply::message(401, "Unauthorized"))?;
    let source = auth
        .get("identitySource")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .unwrap_or("$request.header.Authorization");
    let raw = v2_source(
        source,
        req,
        &route.params,
        &found.path,
        &Value::Null,
        &Value::Null,
    );
    if raw.is_empty() {
        return Err(reply::message(401, "Unauthorized"));
    }
    let token = decode(&raw).ok_or_else(|| refused("token is malformed"))?;
    let config = auth.get("jwtConfiguration").cloned().unwrap_or(Value::Null);
    let issuer = text(&config, "issuer").unwrap_or_default();
    // The keys are the configured issuer's, whatever the token claims: a token from elsewhere fails here or on `iss`.
    signature_ok(jwks, issuer, &token, req.now_ms)
        .await
        .map_err(|why| refused(&why))?;
    if text(&token.claims, "iss") != Some(issuer) {
        return Err(refused("the token has an invalid issuer"));
    }
    if let Some(why) = claims_refuse(&token.claims, &config, req.now_ms / 1000) {
        return Err(refused(&why));
    }
    let have = scopes_of(&token.claims);
    let want = wanted_scopes(route);
    if !want.is_empty() && !want.iter().any(|w| have.contains(w)) {
        let mut r = reply::message(403, "Forbidden");
        r.headers.push((
            "www-authenticate".into(),
            format!("Bearer scope=\"{}\" error=\"insufficient_scope\" error_description=\"the token does not have the required scope\"", want.join(" ")),
        ));
        return Err(r);
    }
    let claims: serde_json::Map<String, Value> = token
        .claims
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.clone(), json!(plain(v))))
        .collect();
    let scopes = if have.is_empty() {
        Value::Null
    } else {
        json!(have)
    };
    extra.authorizer = Some(json!({"jwt": {"claims": claims, "scopes": scopes}}));
    Ok(())
}

/// A REST API's `COGNITO_USER_POOLS` authorizer: the token must come from one of the authorizer's pools (`iss`), be in date, and carry
/// a scope the method asks for (when it asks for any).
pub(crate) fn cognito(
    found: &Found,
    route: &Route,
    req: &Incoming,
    extra: &mut Context,
) -> Result<(), WebReply> {
    let unauthorized = || reply::gateway(found, "UNAUTHORIZED", 401, "Unauthorized", req, None);
    let auth = route.authorizer.as_ref().ok_or_else(unauthorized)?;
    let header = text(auth, "identitySource")
        .and_then(|s| s.strip_prefix("method.request.header."))
        .unwrap_or("Authorization")
        .to_ascii_lowercase();
    let token = req
        .header(&header)
        .and_then(decode)
        .ok_or_else(unauthorized)?;
    let pools: Vec<String> = auth
        .get("providerARNs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|arn| {
            let region = arn.split(':').nth(3)?;
            let pool = arn.rsplit('/').next()?;
            Some(format!("https://cognito-idp.{region}.amazonaws.com/{pool}"))
        })
        .collect();
    let iss = text(&token.claims, "iss").unwrap_or_default();
    if !pools.iter().any(|p| p == iss) || !times_ok(&token.claims, req.now_ms / 1000) {
        return Err(unauthorized());
    }
    let want = wanted_scopes(route);
    if !want.is_empty() && !want.iter().any(|w| scopes_of(&token.claims).contains(w)) {
        return Err(unauthorized());
    }
    let claims: serde_json::Map<String, Value> = token
        .claims
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.clone(), json!(plain(v))))
        .collect();
    extra.authorizer = Some(json!({"claims": claims}));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(v: &Value) -> String {
        aws_core::base64::encode(v.to_string().as_bytes())
            .replace('+', "-")
            .replace('/', "_")
            .trim_end_matches('=')
            .to_owned()
    }

    #[test]
    fn times_are_checked_and_scopes_read() {
        let t = format!(
            "Bearer {}.{}.c2ln",
            enc(&json!({"alg": "RS256"})),
            enc(&json!({"iss": "i", "exp": 100}))
        );
        let token = decode(&t).unwrap();
        assert!(times_ok(&token.claims, 99));
        assert!(!times_ok(&token.claims, 100));
        assert_eq!(scopes_of(&json!({"scope": "a b"})), vec!["a", "b"]);
    }
}
