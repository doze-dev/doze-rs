//! What a broker needs to decide a login: the mechanisms it offers and the users it knows.

use std::sync::Arc;

use crate::credentials::{Credentials, CredentialsError};
use crate::plain::{self, Refusal};
use crate::principal::{Mechanism, Principal};
use crate::scram::{self, Awaiting, Kind, Why};
use crate::users::Users;

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

/// The mechanisms a listener offers and the users that may log in. The mechanisms are fixed once built; the users are
/// two layers (see [`Users`]), the second of which changes at run time behind a snapshot, so a connection never
/// takes a lock to log in.
#[derive(Debug)]
pub struct Authenticator {
    offered: Vec<Mechanism>,
    users: Arc<Users>,
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
        Self::over(
            mechanisms,
            Arc::new(Users::derive(credentials, mechanisms)?),
        )
    }

    /// An authenticator offering `mechanisms` to `users`, which may have no one yet: the broker that shares them
    /// loads the stored users when it opens, and the caller then asks [`Authenticator::ensure_users`].
    ///
    /// # Errors
    /// No mechanism.
    pub fn over(mechanisms: &[Mechanism], users: Arc<Users>) -> Result<Self, SetupError> {
        if mechanisms.is_empty() {
            return Err(SetupError::NoMechanism);
        }
        Ok(Self {
            offered: java_hash_set_order(mechanisms),
            users,
        })
    }

    /// The users, both layers.
    #[must_use]
    pub fn users(&self) -> &Arc<Users> {
        &self.users
    }

    /// Refuses a start where SASL is on and nobody could ever log in: no configured user, and no stored credential
    /// of a SCRAM mechanism that is on (a stored credential cannot log in over PLAIN). There is no way to mean it:
    /// with nobody to log in no super user could ever make the first user over the API (SEC-176).
    ///
    /// # Errors
    /// [`SetupError::NoUsers`].
    pub fn ensure_users(&self) -> Result<(), SetupError> {
        let stored = self.users.stored();
        let stored_can = stored
            .iter()
            .any(|(_, v)| self.offers(v.kind().mechanism()));
        if !self.users.bootstrap().is_empty() || stored_can {
            Ok(())
        } else {
            Err(SetupError::NoUsers)
        }
    }

    /// The mechanisms offered, in the order a SaslHandshake answer lists them: the order Kafka lists them in,
    /// which is not the order they were configured in (see [`java_hash_set_order`]).
    #[must_use]
    pub fn offered(&self) -> &[Mechanism] {
        &self.offered
    }

    /// How many users configured at start may log in (the stored ones come and go).
    #[must_use]
    pub fn user_count(&self) -> usize {
        self.users.bootstrap_len()
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
        scram::first(kind, token, &nonce, |user| self.users.verifier(kind, user))
    }

    /// Judges a PLAIN token. Only the users configured at start can log in this way: a stored SCRAM verifier cannot
    /// give a password back to be compared.
    ///
    /// # Errors
    /// The first check the token fails.
    pub fn plain(&self, token: &[u8]) -> Result<Principal, Refusal> {
        plain::check(token, self.users.bootstrap())
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
