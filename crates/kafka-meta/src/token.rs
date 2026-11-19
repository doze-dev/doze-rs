//! A delegation token as it is kept on disk (KIP-48; Kafka's `DelegationTokenRecord`): whose it is, who may renew it, and
//! its times. Not its HMAC, which is made again from the id and the secret key. A version byte, then postcard, as for
//! the other records.

use serde::{Deserialize, Serialize};

use crate::MetaError;

const VERSION: u8 = 1;

/// One delegation token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRecord {
    /// Its owner, written `Type:name`.
    pub owner: String,
    /// Who asked for it, written `Type:name` (the owner unless it was made for another).
    pub requester: String,
    /// Who besides the owner may renew or expire it, each written `Type:name`.
    pub renewers: Vec<String>,
    /// When it was made, in milliseconds since the epoch.
    pub issue_ms: i64,
    /// The latest it can ever be renewed to.
    pub max_ms: i64,
    /// When it expires unless renewed.
    pub expiry_ms: i64,
}

impl TokenRecord {
    pub(crate) fn encode(&self) -> Result<Vec<u8>, MetaError> {
        postcard::to_extend(self, vec![VERSION])
            .map_err(|e| MetaError::Corrupt(format!("a delegation token cannot be written: {e}")))
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, MetaError> {
        let corrupt = |why: &str| MetaError::Corrupt(format!("delegation token: {why}"));
        let (version, body) = bytes.split_first().ok_or_else(|| corrupt("empty"))?;
        if *version != VERSION {
            return Err(corrupt(&format!("version {version} is not known")));
        }
        postcard::from_bytes(body).map_err(|e| corrupt(&e.to_string()))
    }
}
