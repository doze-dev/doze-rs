//! Delegation tokens as a login sees them (KIP-48): a token's id is the SCRAM user name and the base64 of its HMAC the
//! password, sent with the `tokenauth=true` extension, and the connection is then its owner's.
//!
//! Kafka's `DelegationTokenManager`: the HMAC is HMAC-SHA512 of the token id under `delegation.token.secret.key`, and the
//! SCRAM credential of each mechanism is derived from its base64 at the mechanism's least iteration count (4096) with a
//! fresh salt (`prepareScramCredentials`), when the token is made and again whenever the broker loads it.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::credentials::CredentialsError;
use crate::crypto::{base64, hmac_sha512};
use crate::principal::Principal;
use crate::scram::keys::ITERATIONS;
use crate::scram::{Kind, Verifier};

/// The HMAC of the token `id` under `secret`: what the token's holder proves it has.
#[must_use]
pub fn hmac(secret: &[u8], id: &str) -> [u8; 64] {
    hmac_sha512(secret, &[id.as_bytes()])
}

/// What a login with one token checks: the verifier of each SCRAM mechanism, and whose the token is.
pub struct TokenLogin {
    owner: Principal,
    verifiers: BTreeMap<Kind, Arc<Verifier>>,
}

impl TokenLogin {
    /// The verifiers of the token whose HMAC is `hmac`, owned by `owner`, for every SCRAM mechanism.
    ///
    /// # Errors
    /// [`CredentialsError::NoEntropy`] if the system has no random bytes for the salts.
    pub fn derive(owner: Principal, hmac: &[u8]) -> Result<Self, CredentialsError> {
        let password = base64::encode(hmac);
        let mut verifiers = BTreeMap::new();
        for kind in Kind::ALL {
            let mut salt = [0u8; 16];
            getrandom::fill(&mut salt).map_err(|_| CredentialsError::NoEntropy)?;
            let v = Verifier::derive(kind, password.as_bytes(), &salt, ITERATIONS);
            verifiers.insert(kind, Arc::new(v));
        }
        Ok(Self { owner, verifiers })
    }

    /// The principal a login with the token is: its owner.
    #[must_use]
    pub fn owner(&self) -> &Principal {
        &self.owner
    }

    /// The verifier for `kind`.
    #[must_use]
    pub fn verifier(&self, kind: Kind) -> Option<Arc<Verifier>> {
        self.verifiers.get(&kind).cloned()
    }
}

impl fmt::Debug for TokenLogin {
    /// The owner, and nothing a login is checked with.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenLogin")
            .field("owner", &self.owner)
            .finish_non_exhaustive()
    }
}

/// Every token that can log in, by id. Immutable; a change makes a new one.
#[derive(Clone, Default)]
pub struct TokenLogins(BTreeMap<String, Arc<TokenLogin>>);

impl TokenLogins {
    /// The token `id`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<TokenLogin>> {
        self.0.get(id).cloned()
    }

    /// These and `login` as token `id`'s.
    #[must_use]
    pub fn with(&self, id: &str, login: TokenLogin) -> Self {
        let mut next = self.clone();
        next.0.insert(id.to_owned(), Arc::new(login));
        next
    }

    /// These without the tokens of `ids`.
    #[must_use]
    pub fn without<'a>(&self, ids: impl IntoIterator<Item = &'a str>) -> Self {
        let mut next = self.clone();
        for id in ids {
            next.0.remove(id);
        }
        next
    }

    /// How many tokens can log in.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether no token can.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for TokenLogins {
    /// How many there are; an id is a user name a login can be made with, so none is printed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TokenLogins({})", self.0.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hmac_is_hmac_sha512_of_the_id_under_the_secret() {
        // RFC 4231 test case 2: key "Jefe", data "what do ya want for nothing?".
        let mac = hmac(b"Jefe", "what do ya want for nothing?");
        assert_eq!(mac[..8], [0x16, 0x4b, 0x7a, 0x7b, 0xfc, 0xf8, 0x19, 0xe2]);
    }

    #[test]
    fn a_token_login_has_a_verifier_for_each_mechanism_and_is_its_owners() {
        let login = TokenLogin::derive(Principal::user("alice"), &[7u8; 64]).unwrap();
        assert_eq!(login.owner().as_str(), "User:alice");
        for kind in Kind::ALL {
            assert_eq!(login.verifier(kind).unwrap().iterations(), ITERATIONS);
        }
        let logins = TokenLogins::default().with("t1", login);
        assert_eq!(logins.len(), 1);
        assert!(logins.get("t1").is_some());
        assert!(logins.without(["t1"]).is_empty());
        assert_eq!(format!("{logins:?}"), "TokenLogins(1)");
    }
}
