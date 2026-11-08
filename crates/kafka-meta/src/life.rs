//! What is known of a group's members, kept for the offsets retention. The broker does not keep members across a
//! restart, so it keeps what the retention rules need: that a group has members and the longest session among them,
//! or when it lost the last one, and the protocol type it had. A version byte, then postcard, as for topics.

use serde::{Deserialize, Serialize};

use crate::MetaError;

const VERSION: u8 = 1;

/// A group's membership, as last recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupLife {
    /// It had members when this was recorded. After a restart nobody has joined yet, so they are presumed to be
    /// there until the longest session has run out.
    pub live: bool,
    /// The longest session among its members, in milliseconds.
    pub session_ms: u32,
    /// When it lost its last member, in milliseconds since the epoch; `None` while it has members.
    pub empty_since_ms: Option<i64>,
    /// The kind of group it is (`consumer` for a consumer group); empty if it never had a member.
    pub protocol_type: String,
}

impl GroupLife {
    pub(crate) fn encode(&self) -> Result<Vec<u8>, MetaError> {
        postcard::to_extend(self, vec![VERSION])
            .map_err(|e| MetaError::Corrupt(format!("a group's record cannot be written: {e}")))
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, MetaError> {
        let corrupt = |why: &str| MetaError::Corrupt(format!("group record: {why}"));
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

    #[test]
    fn a_record_is_read_back_whole_and_an_unknown_version_is_refused() {
        let life = GroupLife {
            live: false,
            session_ms: 45_000,
            empty_since_ms: Some(1_700_000_000_000),
            protocol_type: "consumer".into(),
        };
        let mut bytes = life.encode().unwrap();
        assert_eq!(GroupLife::decode(&bytes).unwrap(), life);
        bytes[0] = 9;
        assert!(GroupLife::decode(&bytes).is_err());
        assert!(GroupLife::decode(&[]).is_err());
    }
}
