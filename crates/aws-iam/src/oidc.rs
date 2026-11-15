//! `AssumeRoleWithWebIdentity`'s checks of the token, as AWS makes them: an OpenID Connect provider of the account must be registered
//! for the token's issuer (`iss`); the signature must verify under one of the issuer's keys, fetched from
//! `<iss>/.well-known/openid-configuration` and its `jwks_uri` (RS256/384/512 or ES256/384/512, the STS model's `WebIdentityToken`);
//! the audience (`aud`) must be one of the provider's client ids; and the token must not have expired (`exp`).
//!
//! The signature is checked outside the actor ([`verify`]), because fetching the keys is a network call; the actor ([`Core::claims`])
//! checks the provider, the audience and the time from its own state, and makes the session. A token whose issuer has no provider is
//! refused before anything is fetched, so a token cannot make doze-aws call an address it was not told to trust.
//!
//! **TLS and thumbprints.** AWS: "Amazon Web Services secures communication with OIDC identity providers (IdPs) using our library of
//! trusted root certificate authorities (CAs) to verify the JSON Web Key Set (JWKS) endpoint's TLS certificate. If your OIDC IdP
//! relies on a certificate that is not signed by one of these trusted CAs, only then we secure communication using the thumbprints
//! set in the IdP's configuration" (IAM model, `CreateOpenIDConnectProvider`). Here the keys are fetched over `https://` against the
//! machine's trust store and `--ca-file`; the thumbprint fallback is not built, so a provider whose certificate no trusted authority
//! signed needs its CA in `--ca-file` (or `--insecure-outbound-tls`). The thumbprints are stored and returned, as AWS does.
//!
//! **A local identity provider.** AWS requires the provider URL to begin with `https://`, and it cannot reach this machine anyway. doze
//! also fetches over `http://` when the issuer is on this machine (`localhost`, `127.0.0.0/8`, `[::1]`): register the provider with
//! that same URL (`http://127.0.0.1:8080`) and serve the discovery document and JWKS there. Nothing is accepted unsigned.

use std::time::Duration;

use aws_model_sts::Operation;
use aws_webhook::jwks::Jwks;
use aws_webhook::jwt::{RSA_OR_ECDSA, Refusal, decode};
use aws_wire::ApiError;
use aws_wire::service::Call;
use serde_json::Value;

use crate::Shared;
use crate::core::{Answer, Ask, Core};

/// How long one fetch of an issuer's discovery document or keys may take (memory: AWS does not publish STS's figure).
#[cfg(not(test))]
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
/// Debug-built TLS handshakes under a whole workspace's test load can take longer.
#[cfg(test)]
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// The most bytes of a discovery document or key set read (memory: API Gateway's JWKS quota; STS publishes none).
const MAX_ANSWER: usize = 150_000;

/// The issuers' keys, kept two hours.
pub(crate) fn keys() -> Jwks {
    Jwks::new(FETCH_TIMEOUT, MAX_ANSWER)
}

fn invalid_token(message: impl Into<String>) -> ApiError {
    ApiError::modeled("InvalidIdentityTokenException", message)
}

/// The provider URL an issuer is registered under: without `https://` (as `CreateOpenIDConnectProvider` keeps it) and without a
/// trailing `/`.
fn provider_url(iss: &str) -> &str {
    iss.strip_prefix("https://")
        .unwrap_or(iss)
        .trim_end_matches('/')
}

/// Whether an `http://` issuer is on this machine.
fn local_http(iss: &str) -> bool {
    let Some(rest) = iss.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let host = if authority.starts_with('[') {
        authority
            .split(']')
            .next()
            .map_or("", |h| h.trim_start_matches('['))
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    host == "localhost"
        || host == "::1"
        || host
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// The refusal for a signature that does not verify (memory: the code is the model's; the sentences are doze's, except the one
/// for keys that could not be fetched, which is AWS's as commonly reported).
fn refused(iss: &str, r: &Refusal) -> ApiError {
    match r {
        Refusal::Unfetchable(why) => {
            tracing::warn!(
                "sts: the keys of {iss} could not be fetched, so the web identity token is refused: {why}"
            );
            invalid_token(
                "Couldn't retrieve verification key from your identity provider,  please reference AssumeRoleWithWebIdentity documentation for requirements",
            )
        }
        Refusal::Algorithm(alg) => invalid_token(format!(
            "The token is signed with {alg}: tokens must be signed using RSA keys (RS256, RS384, or RS512) or ECDSA keys (ES256, ES384, or ES512)."
        )),
        Refusal::UnknownKey(kid) => invalid_token(format!(
            "The token's key id {kid} is not among the keys of its identity provider {iss}."
        )),
        Refusal::BadSignature => invalid_token(format!(
            "The token's signature does not verify under the keys of its identity provider {iss}."
        )),
    }
}

/// Before an `AssumeRoleWithWebIdentity` call reaches the actor: when the token's issuer has a provider in the account, its
/// signature is verified against the issuer's keys. Anything else (a token that cannot be read, an issuer with no provider) is
/// left for the actor to refuse in its own words.
pub(crate) async fn verify(shared: &Shared, call: &Call) -> Result<(), ApiError> {
    if aws_model_sts::Operation::from_index(call.op) != Some(Operation::AssumeRoleWithWebIdentity) {
        return Ok(());
    }
    let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
    let raw = body
        .get("WebIdentityToken")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(token) = decode(raw) else {
        return Ok(());
    };
    let iss = token
        .claims
        .get("iss")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let known = shared
        .actor
        .call(Ask::Issuer(provider_url(iss).to_owned()))
        .await;
    if !matches!(known, Ok(Answer::Issuer(true))) {
        return Ok(());
    }
    if !iss.starts_with("https://") && !local_http(iss) {
        return Err(invalid_token(format!(
            "doze-aws fetches an identity provider's keys over https://, or over http:// on this machine (localhost, 127.0.0.1): {iss} is neither."
        )));
    }
    token
        .verify(&shared.jwks, iss, RSA_OR_ECDSA, shared.clock.now_ms())
        .await
        .map_err(|r| refused(iss, &r))
}

impl Core {
    /// Whether an OpenID Connect provider is registered for the issuer at `url` (see [`provider_url`]).
    pub(crate) fn has_issuer(&self, url: &str) -> bool {
        self.oidc_provider(url).is_some()
    }

    fn oidc_provider(&self, url: &str) -> Option<&crate::model::Provider> {
        self.providers
            .iter()
            .map(|(_, p)| p)
            .find(|p| !p.saml && p.url.trim_end_matches('/') == url)
    }

    /// The claims of a web identity token whose signature [`verify`] has checked: its issuer must have a provider, its audience
    /// must be one of the provider's client ids, and it must not have expired. (memory) The sentences for a missing provider, a
    /// wrong audience and an expired token are AWS's as commonly reported, not recorded.
    pub(crate) fn claims(&self, token: &str) -> Result<Value, ApiError> {
        let token = decode(token).ok_or_else(|| {
            invalid_token("The web identity token could not be read: it must be a JSON Web Token (header.claims.signature, base64url).")
        })?;
        let claims = token.claims;
        let text = |k: &str| claims.get(k).and_then(Value::as_str).unwrap_or_default();
        let iss = text("iss");
        let provider = self.oidc_provider(provider_url(iss)).ok_or_else(|| {
            invalid_token(format!(
                "No OpenIDConnect provider found in your account for {iss}"
            ))
        })?;
        let audiences: Vec<&str> = match claims.get("aud") {
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
            Some(Value::String(s)) => vec![s.as_str()],
            _ => Vec::new(),
        };
        if !audiences
            .iter()
            .any(|a| provider.client_ids.iter().any(|c| c == a))
        {
            return Err(invalid_token("Incorrect token audience"));
        }
        let now_secs = self.now_ms() / 1000;
        if let Some(exp) = claims.get("exp").and_then(Value::as_i64)
            && exp <= now_secs
        {
            return Err(ApiError::modeled(
                "ExpiredTokenException",
                format!(
                    "Token expired: current date/time {now_secs} must be before the expiration date/time{exp}"
                ),
            ));
        }
        Ok(claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_issuer_is_matched_without_its_scheme_and_http_only_on_this_machine() {
        assert_eq!(provider_url("https://idp.example.com/"), "idp.example.com");
        assert_eq!(
            provider_url("http://127.0.0.1:8080"),
            "http://127.0.0.1:8080"
        );
        assert!(local_http("http://127.0.0.1:8080/realms/x"));
        assert!(local_http("http://localhost:9000"));
        assert!(local_http("http://[::1]:9000"));
        assert!(!local_http("http://idp.example.com"));
        assert!(!local_http("https://localhost"));
    }
}
