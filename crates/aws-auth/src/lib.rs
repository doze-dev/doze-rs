//! SigV4 request authentication, sans-IO.
//!
//! - [`claim`]: pull the signature claim out of an `Authorization` header or a presigned query string.
//! - [`canonical`]: the canonical request and the string to sign.
//! - [`key`]: key derivation and the HMAC and SHA-256 primitives (RustCrypto).
//! - [`verify`]: the two-step check ([`begin`], then [`Pending::finish`]) and the proof it yields, [`Verified`].
//! - [`chunked`]: the aws-chunked body encoding, verified chunk by chunk.
//! - [`creds`]: the [`Credentials`] a verifier looks secrets up in.
//!
//! Nothing here reads a socket or a body. Ported from spike B (2026-11-08), whose known-answer tests are the worked examples of
//! the S3 documentation; the hand-rolled path costs 1.7 KB where the `aws-sigv4` signer costs 81 KB, and that crate is kept as a
//! test oracle only.

pub mod canonical;
pub mod chunked;
pub mod claim;
pub mod creds;
mod error;
pub mod key;
pub mod verify;

pub use creds::{Credential, CredentialSource, Credentials, Secret, StaticCredentials, WithExtra};
pub use error::AuthError;
pub use verify::{Endpoint, Pending, Scope, Verified, begin};

#[cfg(test)]
mod crosscheck;
#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod known_answer;
