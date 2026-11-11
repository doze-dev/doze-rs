//! Verification, in two steps so the body need not be touched until it must be.
//!
//! [`begin`] checks everything the request head can settle (the claim, the scope, the clock, the credential) and gives a
//! [`Pending`]. If the client signed a payload hash, the signature is then checked at [`Pending::finish`] with no body
//! needed; if it did not (JSON, Query, CBOR and restJson requests), the caller hashes the body and passes the hash in.
//! A client that sent `Expect: 100-continue` with a bad credential is therefore refused before it sends a byte of payload.
//!
//! [`Verified`] is the proof. Its fields are private and only [`Pending::finish`] makes one, so a handler that takes
//! `Verified` cannot be called with anything the signature check did not produce:
//!
//! ```compile_fail,E0451
//! use aws_auth::Verified;
//! let _ = Verified { access_key_id: String::new() };
//! ```
//!
//! ```compile_fail,E0599
//! use aws_auth::Verified;
//! let _ = Verified::default();
//! ```

use http::request::Parts;

use crate::canonical::{Flavor, canonical_request, string_to_sign};
use crate::chunked::ChunkAuth;
use crate::claim::{Claim, PayloadHash, Placement, StreamKind};
use crate::creds::Credentials;
use crate::error::AuthError;
use crate::key::SigningKey;

/// What an endpoint requires of a signature.
#[derive(Debug, Clone, Copy)]
pub struct Endpoint {
    /// The service name the credential scope must carry (`sqs`, `monitoring`, `s3`).
    pub signing_name: &'static str,
    /// S3 canonicalisation: the path is encoded once and never normalised.
    pub s3: bool,
    /// How far the client's clock may be from the server's, in seconds.
    pub max_skew_secs: i64,
}

/// A request whose head has been checked and whose signature is not yet.
#[derive(Debug)]
pub struct Pending {
    claim: Claim,
    key: SigningKey,
    payload: PayloadHash,
    claimed_hash: Option<String>,
    endpoint: Endpoint,
}

/// The region, service and time a request was signed for.
#[derive(Debug, Clone)]
pub struct Scope {
    /// The region in the credential scope.
    pub region: String,
    /// The service in the credential scope.
    pub service: String,
    /// When the client signed, in Unix seconds.
    pub request_time: i64,
}

/// Proof that a request's signature checked out. Not constructible outside this crate.
pub struct Verified {
    access_key_id: String,
    scope: Scope,
    payload: PayloadHash,
    chunk_auth: Option<ChunkAuth>,
}

impl std::fmt::Debug for Verified {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Verified")
            .field("access_key_id", &self.access_key_id)
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}

impl Verified {
    /// The access key that signed.
    #[must_use]
    pub fn access_key_id(&self) -> &str {
        &self.access_key_id
    }

    /// The region, service and time it was signed for.
    #[must_use]
    pub const fn scope(&self) -> &Scope {
        &self.scope
    }

    /// What the signature promised about the body.
    #[must_use]
    pub const fn payload(&self) -> &PayloadHash {
        &self.payload
    }

    /// For an `aws-chunked` body signed chunk by chunk: the context to verify the chunks with. `None` otherwise.
    pub fn take_chunk_auth(&mut self) -> Option<ChunkAuth> {
        self.chunk_auth.take()
    }
}

/// Checks the head of a request: the claim, the scope against `endpoint`, the clock against `now`, and the credential.
///
/// # Errors
/// Any [`AuthError`] that does not need the body.
pub fn begin<C: Credentials>(
    parts: &Parts,
    now: i64,
    endpoint: Endpoint,
    creds: &C,
) -> Result<Pending, AuthError> {
    let claim = Claim::parse(parts)?.ok_or(AuthError::Missing)?;
    if claim.service != endpoint.signing_name {
        return Err(AuthError::ScopeMismatch {
            scope: claim.service,
            expected: endpoint.signing_name,
        });
    }
    check_time(&claim, now, endpoint.max_skew_secs)?;
    let presigned = claim.placement == Placement::Query;
    let (payload, claimed) =
        PayloadHash::classify(claim.content_sha256.as_deref(), presigned && endpoint.s3)?;
    let claimed_hash = claimed.map(str::to_owned);
    let key = creds.lookup(&claim.access_key_id, |c| {
        let c = c.ok_or_else(|| AuthError::UnknownAccessKey(claim.access_key_id.clone()))?;
        if c.expires_at.is_some_and(|t| t <= now) {
            return Err(AuthError::ExpiredToken);
        }
        if c.session_token.as_deref() != claim.session_token.as_deref() {
            return Err(AuthError::InvalidToken);
        }
        SigningKey::derive(
            c.secret.expose(),
            &claim.scope_date,
            &claim.region,
            &claim.service,
        )
        .ok_or(AuthError::Internal)
    })?;
    Ok(Pending {
        claim,
        key,
        payload,
        claimed_hash,
        endpoint,
    })
}

impl Pending {
    /// The service named in the credential scope: a routing key, untrusted until [`finish`](Self::finish).
    #[must_use]
    pub fn claimed_service(&self) -> &str {
        &self.claim.service
    }

    /// True when the client did not sign a payload hash, so the caller must hash the body and pass it to `finish`.
    #[must_use]
    pub const fn needs_body_hash(&self) -> bool {
        matches!(self.payload, PayloadHash::Absent)
    }

    /// Checks the signature. `body_sha256_hex` is the hex SHA-256 of the whole body when [`needs_body_hash`] says so.
    ///
    /// [`needs_body_hash`]: Self::needs_body_hash
    ///
    /// # Errors
    /// [`AuthError::SignatureMismatch`] with the strings S3 reports, or `Internal` if a needed hash was not given.
    pub fn finish(
        self,
        parts: &Parts,
        body_sha256_hex: Option<&str>,
    ) -> Result<Verified, AuthError> {
        let hash = self
            .claimed_hash
            .as_deref()
            .or(body_sha256_hex)
            .ok_or(AuthError::Internal)?;
        let scope = check_signature(parts, &self.claim, &self.key, hash, self.endpoint.s3)?;
        let chunk_auth = match self.payload {
            PayloadHash::Streaming(kind) if kind != StreamKind::UnsignedTrailer => {
                Some(ChunkAuth {
                    key: self.key,
                    amz_date: self.claim.amz_date.clone(),
                    scope,
                    prev_signature: self.claim.signature_hex.clone(),
                })
            }
            _ => None,
        };
        Ok(Verified {
            access_key_id: self.claim.access_key_id,
            scope: Scope {
                region: self.claim.region,
                service: self.claim.service,
                request_time: self.claim.request_time,
            },
            payload: self.payload,
            chunk_auth,
        })
    }
}

/// Rebuilds the canonical request and string to sign, and compares. Returns the credential-scope string.
pub(crate) fn check_signature(
    parts: &Parts,
    claim: &Claim,
    key: &SigningKey,
    payload_hash: &str,
    s3: bool,
) -> Result<String, AuthError> {
    let flavor = Flavor {
        s3,
        presigned: claim.placement == Placement::Query,
    };
    let canonical = canonical_request(parts, &claim.signed_headers, payload_hash, flavor)?;
    let scope = claim.scope();
    let sts = string_to_sign(&claim.amz_date, &scope, &canonical);
    if key.verify(sts.as_bytes(), &claim.signature) {
        Ok(scope)
    } else {
        Err(AuthError::SignatureMismatch {
            access_key_id: claim.access_key_id.clone(),
            signature_provided: claim.signature_hex.clone(),
            canonical_request: canonical,
            string_to_sign: sts,
        })
    }
}

fn check_time(claim: &Claim, now: i64, max: i64) -> Result<(), AuthError> {
    let skew = (now - claim.request_time).abs();
    let skewed = || AuthError::Skewed {
        request_time: claim.request_time,
        server_time: now,
        skew,
        max,
    };
    match claim.expires_in {
        // Presigned: valid from (request time - skew) until request time + lifetime.
        Some(expires_in) => {
            if claim.request_time - now > max {
                return Err(skewed());
            }
            if now > claim.request_time + expires_in {
                return Err(AuthError::PresignExpired {
                    request_time: claim.request_time,
                    expires_in,
                    server_time: now,
                });
            }
        }
        None if skew > max => return Err(skewed()),
        None => {}
    }
    Ok(())
}
