//! JSON Web Tokens: an HTTP API's `JWT` authorizer and a REST API's `COGNITO_USER_POOLS` authorizer. A token is read, its issuer,
//! audience (or `client_id`), expiry and not-before are checked, and the route's scopes against its `scope`/`scp`. The signature is
//! verified (RS256) against the issuer's JWKS when the issuer can be reached over `http://` (a local identity provider); an `https://`
//! issuer cannot be fetched without a TLS client, and Cognito is not served here, so for those the claims are checked and the
//! signature is not (said in doze-aws's log, once per issuer).

use std::time::Duration;

use aws_wire::service::WebReply;
use serde_json::{Value, json};

use super::event::Context;
use super::mapping::{plain, v2_source};
use super::{Found, Incoming, Route, client, reply};
use crate::doc::text;

/// A token taken apart.
#[derive(Debug)]
pub(crate) struct Token {
    pub(crate) header: Value,
    pub(crate) claims: Value,
    signed: String,
    signature: Vec<u8>,
}

fn b64url(s: &str) -> Option<Vec<u8>> {
    let mut t = s.replace('-', "+").replace('_', "/");
    while t.len() % 4 != 0 {
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

/// Whether an RS256 signature verifies under the JWK `{kty: RSA, n, e}`.
pub(crate) fn rs256(jwk: &Value, t: &Token) -> bool {
    let (Some(n), Some(e)) = (
        text(jwk, "n").and_then(b64url),
        text(jwk, "e").and_then(b64url),
    ) else {
        return false;
    };
    let bits = |b: &[u8]| u32::try_from(b.len() * 8).unwrap_or(0);
    let (Ok(n), Ok(e)) = (
        rsa::BoxedUint::from_be_slice(&n, bits(&n)),
        rsa::BoxedUint::from_be_slice(&e, bits(&e).max(64)),
    ) else {
        return false;
    };
    rsa::PublicKey::new(n, e).is_ok_and(|key| {
        key.pkcs1v15_verify::<rsa::sha2::Sha256>(t.signed.as_bytes(), &t.signature)
            .is_ok()
    })
}

/// The issuer's keys, from `<issuer>/.well-known/openid-configuration` and its `jwks_uri`; `None` when they cannot be fetched.
async fn jwks(issuer: &str) -> Option<Vec<Value>> {
    let get = |url: String| client::Out {
        method: "GET".into(),
        url,
        headers: Vec::new(),
        body: bytes::Bytes::new(),
        timeout: Duration::from_secs(5),
    };
    let config = client::call(&get(format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    )))
    .await
    .ok()?;
    let config: Value = serde_json::from_slice(&config.body).ok()?;
    let keys = client::call(&get(text(&config, "jwks_uri")?.to_owned()))
        .await
        .ok()?;
    let keys: Value = serde_json::from_slice(&keys.body).ok()?;
    keys.get("keys").and_then(Value::as_array).cloned()
}

/// Whether the signature verifies: `Some(false)` when it does not, `None` when the issuer's keys cannot be had.
async fn signature_ok(issuer: &str, t: &Token) -> Option<bool> {
    if !issuer.starts_with("http://") {
        tracing::info!(
            "JWT from {issuer}: claims checked, signature not verified (no TLS client to fetch its keys)"
        );
        return None;
    }
    let keys = jwks(issuer).await?;
    let kid = text(&t.header, "kid");
    let alg = text(&t.header, "alg").unwrap_or_default();
    if alg != "RS256" {
        return Some(false);
    }
    Some(
        keys.iter()
            .filter(|k| kid.is_none() || text(k, "kid") == kid)
            .any(|k| rs256(k, t)),
    )
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
    found: &Found,
    route: &Route,
    req: &Incoming,
    extra: &mut Context,
) -> Result<(), WebReply> {
    let unauthorized = || reply::message(401, "Unauthorized");
    let auth = route.authorizer.as_ref().ok_or_else(unauthorized)?;
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
    let token = decode(&raw).ok_or_else(unauthorized)?;
    let config = auth.get("jwtConfiguration").cloned().unwrap_or(Value::Null);
    let issuer = text(&config, "issuer").unwrap_or_default();
    if text(&token.claims, "iss") != Some(issuer) || !times_ok(&token.claims, req.now_ms / 1000) {
        return Err(unauthorized());
    }
    let audience: Vec<String> = config
        .get("audience")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(plain).collect())
        .unwrap_or_default();
    let aud: Vec<String> = match token.claims.get("aud") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().map(plain).collect(),
        _ => text(&token.claims, "client_id")
            .map(|c| vec![c.to_owned()])
            .unwrap_or_default(),
    };
    if !audience.is_empty() && !aud.iter().any(|a| audience.contains(a)) {
        return Err(unauthorized());
    }
    if signature_ok(issuer, &token).await == Some(false) {
        return Err(unauthorized());
    }
    let have = scopes_of(&token.claims);
    let want = wanted_scopes(route);
    if !want.is_empty() && !want.iter().any(|w| have.contains(w)) {
        return Err(reply::message(403, "Forbidden"));
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
