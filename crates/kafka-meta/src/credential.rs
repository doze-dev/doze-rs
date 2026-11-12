//! A SCRAM credential as it is kept on disk: what a server needs to check a login, and nothing a login could be
//! made from. A version byte, then postcard, as for ACLs.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::MetaError;

const VERSION: u8 = 1;

/// Whose credential, and for which mechanism: the key of the table. The mechanism is the number Kafka's
/// `AlterUserScramCredentials` carries (1 is SCRAM-SHA-256, 2 is SCRAM-SHA-512); this crate does not judge it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CredentialId {
    /// The user's name, as the client wrote it.
    pub user: String,
    /// The mechanism, as on the wire.
    pub mechanism: i8,
}

impl CredentialId {
    /// The mechanism first, then the name's bytes: no name can run into the mechanism, whatever bytes it holds.
    pub(crate) fn key(&self) -> Vec<u8> {
        let mut key = Vec::with_capacity(1 + self.user.len());
        key.extend_from_slice(&self.mechanism.to_be_bytes());
        key.extend_from_slice(self.user.as_bytes());
        key
    }

    pub(crate) fn from_key(key: &[u8]) -> Result<Self, MetaError> {
        let corrupt = |why: &str| MetaError::Corrupt(format!("credential key: {why}"));
        let (mechanism, user) = key.split_first().ok_or_else(|| corrupt("empty"))?;
        let user = std::str::from_utf8(user).map_err(|_| corrupt("the name is not text"))?;
        Ok(Self {
            user: user.to_owned(),
            mechanism: i8::from_be_bytes([*mechanism]),
        })
    }
}

/// One user's verifier for one mechanism: the salt and the iteration count the client is sent, and the two keys the
/// server checks a proof and signs its answer with. The password, and the salted password it is made from, are never
/// kept.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRecord {
    /// The salt, as the client that set the credential chose it.
    pub salt: Vec<u8>,
    /// The iteration count.
    pub iterations: u32,
    /// `H(ClientKey)`.
    pub stored_key: Vec<u8>,
    /// `HMAC(SaltedPassword, "Server Key")`.
    pub server_key: Vec<u8>,
}

impl fmt::Debug for CredentialRecord {
    /// The iteration count and nothing else: not a key, and not the salt.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialRecord")
            .field("iterations", &self.iterations)
            .finish_non_exhaustive()
    }
}

impl CredentialRecord {
    pub(crate) fn encode(&self) -> Result<Vec<u8>, MetaError> {
        postcard::to_extend(self, vec![VERSION])
            .map_err(|e| MetaError::Corrupt(format!("a credential cannot be written: {e}")))
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, MetaError> {
        let corrupt = |why: &str| MetaError::Corrupt(format!("credential record: {why}"));
        let (version, body) = bytes.split_first().ok_or_else(|| corrupt("empty"))?;
        if *version != VERSION {
            return Err(corrupt(&format!("version {version} is not known")));
        }
        postcard::from_bytes(body).map_err(|e| corrupt(&e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> CredentialRecord {
        CredentialRecord {
            salt: b"salty".to_vec(),
            iterations: 4096,
            stored_key: vec![1; 32],
            server_key: vec![2; 32],
        }
    }

    #[test]
    fn a_record_is_read_back_whole_and_an_unknown_version_is_refused() {
        let r = record();
        let bytes = r.encode().unwrap();
        assert_eq!(bytes.first(), Some(&VERSION));
        assert_eq!(CredentialRecord::decode(&bytes).unwrap(), r);
        let mut other = bytes.clone();
        other[0] = 9;
        assert!(CredentialRecord::decode(&other).is_err());
        assert!(CredentialRecord::decode(&[]).is_err());
        assert!(CredentialRecord::decode(&bytes[..bytes.len() - 1]).is_err());
    }

    #[test]
    fn a_key_is_read_back_and_tells_apart_users_and_mechanisms() {
        let id = |user: &str, mechanism| CredentialId {
            user: user.into(),
            mechanism,
        };
        let all = [
            id("alice", 1),
            id("alice", 2),
            id("bob", 1),
            id("", 1),
            id("a\0b", 1),
            id("\u{1}", 2),
            id("é", -1),
        ];
        let mut seen = std::collections::HashSet::new();
        for one in &all {
            assert!(seen.insert(one.key()), "{one:?}");
            assert_eq!(&CredentialId::from_key(&one.key()).unwrap(), one);
        }
        assert!(CredentialId::from_key(&[]).is_err());
        assert!(CredentialId::from_key(&[1, 0xff]).is_err());
    }

    #[test]
    fn a_record_prints_no_secret() {
        let shown = format!("{:?}", record());
        assert!(shown.contains("4096"));
        assert!(
            !shown.contains("salty") && !shown.contains("115"),
            "{shown}"
        );
        assert!(!shown.contains("[1, 1"), "{shown}");
    }
}
