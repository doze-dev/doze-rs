//! The keys of SCRAM (RFC 5802 section 3), for SHA-256 and SHA-512.
//!
//! A [`Verifier`] is all a server keeps of a password: the salt, the iteration count, `StoredKey = H(ClientKey)`
//! and `ServerKey`. With them it can check a client's proof and sign its own answer, and a stolen verifier does
//! not let anyone log in (the proof needs the `ClientKey`, which is not in it).

use std::fmt;
use std::num::NonZeroU32;

use crate::crypto::{self, pbkdf2};
use crate::principal::Mechanism;

/// The iteration count of every verifier made here: the least Apache Kafka accepts (4096), and the one recorded.
pub const ITERATIONS: NonZeroU32 = match NonZeroU32::new(4096) {
    Some(n) => n,
    None => NonZeroU32::MIN,
};

/// Which hash a SCRAM mechanism is built on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// `SCRAM-SHA-256`: SHA-256, 32-byte keys and proofs.
    Sha256,
    /// `SCRAM-SHA-512`: SHA-512, 64-byte keys and proofs.
    Sha512,
}

impl Kind {
    /// Both kinds.
    pub const ALL: [Kind; 2] = [Self::Sha256, Self::Sha512];

    /// The kind of `mechanism`, or `None` for PLAIN.
    #[must_use]
    pub const fn of(mechanism: Mechanism) -> Option<Self> {
        match mechanism {
            Mechanism::Plain => None,
            Mechanism::ScramSha256 => Some(Self::Sha256),
            Mechanism::ScramSha512 => Some(Self::Sha512),
        }
    }

    /// The mechanism this kind is the hash of.
    #[must_use]
    pub const fn mechanism(self) -> Mechanism {
        match self {
            Self::Sha256 => Mechanism::ScramSha256,
            Self::Sha512 => Mechanism::ScramSha512,
        }
    }

    /// The size of a hash, a key and a proof, in bytes.
    #[must_use]
    pub const fn hash_len(self) -> usize {
        match self {
            Self::Sha256 => 32,
            Self::Sha512 => 64,
        }
    }

    /// The hash of `data`.
    #[must_use]
    pub fn hash(self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::Sha256 => crypto::sha256(data).to_vec(),
            Self::Sha512 => crypto::sha512(data).to_vec(),
        }
    }

    /// HMAC of the concatenation of `parts` under `key`.
    #[must_use]
    pub fn hmac(self, key: &[u8], parts: &[&[u8]]) -> Vec<u8> {
        match self {
            Self::Sha256 => crypto::hmac_sha256(key, parts).to_vec(),
            Self::Sha512 => crypto::hmac_sha512(key, parts).to_vec(),
        }
    }

    /// `Hi(password, salt, iterations)` of RFC 5802: PBKDF2 with this hash's HMAC, one block.
    #[must_use]
    pub fn salted_password(self, password: &[u8], salt: &[u8], iterations: NonZeroU32) -> Vec<u8> {
        match self {
            Self::Sha256 => pbkdf2::sha256(password, salt, iterations).to_vec(),
            Self::Sha512 => pbkdf2::sha512(password, salt, iterations).to_vec(),
        }
    }
}

/// What a server keeps of one user's password for one mechanism. It cannot be printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Verifier {
    kind: Kind,
    salt: Box<[u8]>,
    iterations: NonZeroU32,
    stored_key: Box<[u8]>,
    server_key: Box<[u8]>,
}

impl Verifier {
    /// The verifier of `password` under `salt` and `iterations`.
    #[must_use]
    pub fn derive(kind: Kind, password: &[u8], salt: &[u8], iterations: NonZeroU32) -> Self {
        let salted = kind.salted_password(password, salt, iterations);
        let client_key = kind.hmac(&salted, &[b"Client Key"]);
        Self {
            kind,
            salt: salt.into(),
            iterations,
            stored_key: kind.hash(&client_key).into(),
            server_key: kind.hmac(&salted, &[b"Server Key"]).into(),
        }
    }

    /// The hash this verifier is for.
    #[must_use]
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The salt, which the client is sent.
    #[must_use]
    pub fn salt(&self) -> &[u8] {
        &self.salt
    }

    /// The iteration count, which the client is sent.
    #[must_use]
    pub fn iterations(&self) -> NonZeroU32 {
        self.iterations
    }

    /// Whether `proof` is the `ClientProof` of the right password for `auth_message`: the client signs the
    /// message with its `StoredKey`-derived signature and hides the `ClientKey` in the XOR, so XOR-ing the
    /// signature off and hashing must give `StoredKey` back. The comparison takes the same time wherever the
    /// two differ. A proof of another length than the hash is never right.
    #[must_use]
    pub fn proof_is_right(&self, auth_message: &[u8], proof: &[u8]) -> bool {
        let signature = self.kind.hmac(&self.stored_key, &[auth_message]);
        if proof.len() != signature.len() {
            return false;
        }
        let client_key: Vec<u8> = signature.iter().zip(proof).map(|(s, p)| s ^ p).collect();
        crypto::ct_eq(&self.kind.hash(&client_key), &self.stored_key)
    }

    /// `ServerSignature = HMAC(ServerKey, AuthMessage)`: what the client checks to know the server also knew
    /// the password.
    #[must_use]
    pub fn server_signature(&self, auth_message: &[u8]) -> Vec<u8> {
        self.kind.hmac(&self.server_key, &[auth_message])
    }
}

impl fmt::Debug for Verifier {
    /// The kind and the cost; never a key, and not the salt.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Verifier")
            .field("kind", &self.kind)
            .field("iterations", &self.iterations)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
