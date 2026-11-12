//! What a broker needs to decide a login: the mechanisms it offers and the users it knows.

use crate::credentials::{Credentials, CredentialsError};
use crate::plain::{self, Refusal};
use crate::principal::{Mechanism, Principal};
use crate::scram::{self, Awaiting, Kind, Why};

/// A configuration that cannot work. The text never holds a password.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SetupError {
    /// No mechanism was given.
    #[error("no SASL mechanism is enabled")]
    NoMechanism,
    /// SASL is on and nobody can log in.
    #[error("SASL is enabled but no credentials are configured")]
    NoUsers,
    /// The verifiers for the SCRAM mechanisms could not be made.
    #[error("{0}")]
    Scram(#[from] CredentialsError),
}

/// The mechanisms a listener offers and the users that may log in. Immutable once built, so it is shared by
/// every connection without a lock.
#[derive(Debug)]
pub struct Authenticator {
    offered: Vec<Mechanism>,
    credentials: Credentials,
}

impl Authenticator {
    /// An authenticator offering `mechanisms` to the users in `credentials`.
    ///
    /// # Errors
    /// No mechanism, no user, or no random bytes for the salts of the SCRAM verifiers.
    pub fn new(mechanisms: &[Mechanism], credentials: Credentials) -> Result<Self, SetupError> {
        if mechanisms.is_empty() {
            return Err(SetupError::NoMechanism);
        }
        if credentials.is_empty() {
            return Err(SetupError::NoUsers);
        }
        let kinds: Vec<Kind> = mechanisms.iter().filter_map(|&m| Kind::of(m)).collect();
        let credentials = credentials.with_scram(&kinds)?;
        Ok(Self {
            offered: java_hash_set_order(mechanisms),
            credentials,
        })
    }

    /// The mechanisms offered, in the order a SaslHandshake answer lists them: the order Kafka lists them in,
    /// which is not the order they were configured in (see [`java_hash_set_order`]).
    #[must_use]
    pub fn offered(&self) -> &[Mechanism] {
        &self.offered
    }

    /// How many users may log in.
    #[must_use]
    pub fn user_count(&self) -> usize {
        self.credentials.len()
    }

    /// Whether `mechanism` is offered.
    #[must_use]
    pub fn offers(&self, mechanism: Mechanism) -> bool {
        self.offered.contains(&mechanism)
    }

    /// Reads the client-first `token` of a SCRAM exchange for `mechanism` and answers with the server-first
    /// message (the first element) and the exchange to carry on with. The server nonce is fresh.
    ///
    /// # Errors
    /// The first check the token fails; PLAIN and a mechanism that is not offered are refused too.
    pub fn scram_first(
        &self,
        mechanism: Mechanism,
        token: &[u8],
    ) -> Result<(Vec<u8>, Awaiting), scram::Refusal> {
        let rejected = |why| scram::Refusal::Rejected { mechanism, why };
        let kind = Kind::of(mechanism).filter(|_| self.offers(mechanism));
        let kind = kind.ok_or(rejected(Why::UnknownUser))?;
        let nonce = scram::fresh_nonce().ok_or(rejected(Why::NoEntropy))?;
        scram::first(kind, token, &nonce, |user| {
            self.credentials.verifier(kind, user)
        })
    }

    /// Judges a PLAIN token.
    ///
    /// # Errors
    /// The first check the token fails.
    pub fn plain(&self, token: &[u8]) -> Result<Principal, Refusal> {
        plain::check(token, &self.credentials)
    }
}

/// `mechanisms` without repeats, in the order `java.util.HashSet<String>` iterates them.
///
/// Kafka builds the list it answers a SaslHandshake with as `new ArrayList<>(new HashSet<>(configured))`
/// (`SaslServerAuthenticator`, 4.3.1), so the answer follows the hash table and not the configuration: a
/// table of 16 buckets, a string's bucket being `h ^ (h >>> 16)` masked to four bits for its `String.hashCode`
/// `h`, and a bucket keeping its members in the order they were added. For the three mechanisms, `PLAIN` and
/// `SCRAM-SHA-512` share bucket 0 and `SCRAM-SHA-256` is in bucket 6, so the full list is
/// `PLAIN, SCRAM-SHA-512, SCRAM-SHA-256` (recorded) whatever the order it was configured in, unless
/// `SCRAM-SHA-512` was written before `PLAIN`.
#[must_use]
pub fn java_hash_set_order(mechanisms: &[Mechanism]) -> Vec<Mechanism> {
    let mut out: Vec<Mechanism> = Vec::with_capacity(mechanisms.len());
    for &m in mechanisms {
        if !out.contains(&m) {
            out.push(m);
        }
    }
    out.sort_by_key(|m| bucket(m.name()));
    out
}

/// The bucket of `name` in a Java `HashMap` of 16 buckets.
fn bucket(name: &str) -> u32 {
    let hash = name
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    (hash ^ (hash >> 16)) & 15
}

#[cfg(test)]
mod tests;
