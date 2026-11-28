//! The signature on a request to an address a service owns, for a service that asks (a Lambda function URL with `AuthType`
//! `AWS_IAM`): checked as an API call's would be, before the service sees the request.
//!
//! Under `--auth verify` every signature is checked with the secret (the configured key, or one IAM issued). Under `--auth trust`
//! a signature made with a key whose secret is known anyway (the configured key, one IAM issued) is checked too, so a request a
//! client signed wrongly is refused as AWS refuses it; one made with any other key is taken on trust, as every API call is under
//! trust, once its scope (the service's signing name) and its time are checked.

use aws_auth::claim::Claim;
use aws_auth::{AuthError, Credential, Credentials, Endpoint, Secret, WithExtra, begin_claimed};
use aws_wire::service::WebSignature;
use aws_wire::{Family, from_auth};
use bytes::Bytes;
use doze_runtime::{Clock, SystemClock};
use http::request::Parts;

use crate::Auth;
use crate::pipeline::Shared;

/// What the signature on the request comes to, for a service whose signing name is `signing_name`.
pub(crate) async fn signature_of(
    shared: &Shared,
    parts: &Parts,
    signing_name: &'static str,
    body: &Bytes,
) -> WebSignature {
    let claim = match Claim::parse(parts) {
        Ok(Some(claim)) => claim,
        Ok(None) => return WebSignature::Unsigned,
        Err(e) => return refused(&e),
    };
    let (fixed, verifying) = match &shared.config.auth {
        Auth::Verify(creds) => (creds.as_ref(), true),
        Auth::Trust => (shared.config.known_keys.as_ref(), false),
    };
    let mut extra = None;
    if let Some(source) = &shared.config.credential_source
        && fixed.lookup(&claim.access_key_id, |c| c.is_none())
    {
        extra = source
            .find(&claim.access_key_id, claim.session_token.as_deref())
            .await
            .map(|c| (claim.access_key_id.clone(), c));
    }
    let known = WithExtra {
        fixed,
        extra: extra.as_ref().map(|(id, c)| (id.as_str(), c)),
    };
    let endpoint = Endpoint {
        signing_name,
        s3: false,
        max_skew_secs: shared.config.max_skew_secs,
    };
    let now = SystemClock.now_ms() / 1000;
    let checked = known.lookup(&claim.access_key_id, |c| c.is_some());
    let signed = WebSignature::Signed {
        access_key_id: claim.access_key_id.clone(),
        session_token: claim.session_token.clone(),
        region: claim.region.clone(),
        verified: checked,
    };
    if !checked && !verifying {
        // A key nobody here knows: everything but the signature itself is checked, with a stand-in secret that is never compared.
        let anyone = Anyone(Credential {
            secret: Secret::new(String::new()),
            session_token: claim.session_token.clone(),
            expires_at: None,
        });
        return match begin_claimed(&claim, now, endpoint, &anyone) {
            Ok(_) => signed,
            Err(e) => refused(&e),
        };
    }
    let finished = begin_claimed(&claim, now, endpoint, &known).and_then(|pending| {
        let hash = pending
            .needs_body_hash()
            .then(|| aws_auth::key::sha256_hex(body));
        pending.finish(parts, hash.as_deref())
    });
    match finished {
        Ok(_) => signed,
        Err(e) => refused(&e),
    }
}

/// AWS's words for the failure (as the JSON services word it).
fn refused(e: &AuthError) -> WebSignature {
    WebSignature::Refused(from_auth(e, Family::Coral).message)
}

/// Every key, with one stand-in credential.
struct Anyone(Credential);

impl Credentials for Anyone {
    fn lookup<R>(&self, _access_key_id: &str, f: impl FnOnce(Option<&Credential>) -> R) -> R {
        f(Some(&self.0))
    }
}
