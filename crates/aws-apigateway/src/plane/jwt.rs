//! JSON Web Tokens: an HTTP API's `JWT` authorizer and a REST API's `COGNITO_USER_POOLS` authorizer.
//!
//! **JWT** (HTTP APIs), in AWS's order: the token is read; it must be signed with an RSA algorithm ("Currently, only RSA-based
//! algorithms are supported": RS256, RS384, RS512; an ES256 or HS256 token is refused); the signature is verified against the issuer's
//! JWKS (`jwks.rs`: fetched over `https://` or `http://`, kept two hours); then `iss`, `exp`, `nbf`, the audience (`aud`, else
//! `client_id`) and the route's scopes (`scope`/`scp`). A refusal is 401 `{"message":"Unauthorized"}` with a
//! `www-authenticate: Bearer scope="" error="invalid_token" error_description="…"` header, a missing scope 403 `Forbidden` with
//! `error="insufficient_scope"` (the header and its descriptions are (memory)). When the keys cannot be fetched (the issuer is down,
//! or its certificate is refused), the token is refused, as AWS refuses it, and doze-aws's log says why (with the flag that fixes a
//! refused certificate).
//!
//! **COGNITO_USER_POOLS** (REST APIs): the claims are checked; the signature is not, because Cognito is not served here and a token
//! from a local test cannot be signed by a real pool.

use aws_wire::service::WebReply;
use serde_json::{Value, json};

use super::event::Context;
use super::jwks::{Jwks, rsa_verifies};
use super::mapping::{plain, v2_source};
use super::{Found, Incoming, Route, reply};
use crate::doc::text;

/// A token taken apart.
#[derive(Debug)]
pub(crate) struct Token {
    pub(crate) header: Value,
    pub(crate) claims: Value,
    signed: String,
    signature: Vec<u8>,
}

pub(crate) fn b64url(s: &str) -> Option<Vec<u8>> {
    let mut t = s.replace('-', "+").replace('_', "/");
    while !t.len().is_multiple_of(4) {
        t.push('=');
    }
    aws_core::base64::decode(&t)
}

// fuzz:parser fuzz_jwt
/// Reads a token (`header.claims.signature`, base64url).
pub(crate) fn decode(token: &str) -> Option<Token> {
    let token = token.strip_prefix("Bearer ").unwrap_or(token).trim();
    let mut parts = token.split('.');
    let (h, c, s) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || token.len() > 16 * 1024 {
        return None;
    }
    Some(Token {
        header: serde_json::from_slice(&b64url(h)?).ok()?,
        claims: serde_json::from_slice(&b64url(c)?).ok()?,
        signed: format!("{h}.{c}"),
        signature: b64url(s)?,
    })
}

/// Why a token is refused: the `error_description` of the `www-authenticate` header (memory).
type Refusal = String;

/// Whether the signature verifies under one of the issuer's keys.
async fn signature_ok(jwks: &Jwks, issuer: &str, t: &Token, now_ms: i64) -> Result<(), Refusal> {
    let alg = text(&t.header, "alg").unwrap_or_default();
    if !matches!(alg, "RS256" | "RS384" | "RS512") {
        return Err(format!("signing method {alg} is invalid"));
    }
    let kid = text(&t.header, "kid");
    let keys = jwks.keys(issuer, kid, now_ms).await.map_err(|why| {
        tracing::warn!("JWT authorizer: the keys of {issuer} could not be fetched, so the token is refused: {why}");
        "unable to fetch the issuer's public keys".to_owned()
    })?;
    let mut candidates = keys
        .iter()
        .filter(|k| kid.is_none() || text(k, "kid") == kid)
        .peekable();
    if candidates.peek().is_none() {
        return Err(format!(
            "the key id {} is not in the issuer's JWKS",
            kid.unwrap_or_default()
        ));
    }
    if candidates.any(|k| rsa_verifies(k, alg, t.signed.as_bytes(), &t.signature)) {
        Ok(())
    } else {
        Err("the token's signature is invalid".to_owned())
    }
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
fn claims_refuse(claims: &Value, config: &Value, now_secs: i64) -> Option<Refusal> {
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
    if text(&token.claims, "iss") != Some(issuer) {
        return Err(refused("the token has an invalid issuer"));
    }
    signature_ok(jwks, issuer, &token, req.now_ms)
        .await
        .map_err(|why| refused(&why))?;
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
    fn tokens_are_taken_apart_and_their_times_checked() {
        let t = format!(
            "Bearer {}.{}.c2ln",
            enc(&json!({"alg": "RS256"})),
            enc(&json!({"iss": "i", "exp": 100}))
        );
        let token = decode(&t).unwrap();
        assert_eq!(token.claims["iss"], "i");
        assert!(times_ok(&token.claims, 99));
        assert!(!times_ok(&token.claims, 100));
        assert!(decode("a.b").is_none());
        assert!(decode("!!.!!.!!").is_none());
        assert_eq!(scopes_of(&json!({"scope": "a b"})), vec!["a", "b"]);
    }
}
