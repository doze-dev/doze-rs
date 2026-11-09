//! What is known of a group's members, kept for the offsets retention. The broker does not keep members across a
//! restart, so it keeps what the retention rules need: that a group has members and the longest session among them,
//! or when it lost the last one, and the protocol type it had. A version byte, then postcard, as for topics.

use serde::{Deserialize, Serialize};

use crate::MetaError;

const VERSION: u8 = 2;

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
    /// Set for a group of the new protocol (KIP-848): what is needed to list and describe it again after a restart.
    pub consumer: Option<ConsumerLife>,
}

/// What a consumer group of the new protocol keeps across a restart: its epochs and its assignor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumerLife {
    /// The group epoch.
    pub epoch: i32,
    /// The epoch the members' targets were last computed for.
    pub assignment_epoch: i32,
    /// The assignor in use.
    pub assignor: String,
}

/// A record as the first version wrote it, before consumer groups.
#[derive(Deserialize)]
struct V1 {
    live: bool,
    session_ms: u32,
    empty_since_ms: Option<i64>,
    protocol_type: String,
}

impl GroupLife {
    pub(crate) fn encode(&self) -> Result<Vec<u8>, MetaError> {
        postcard::to_extend(self, vec![VERSION])
            .map_err(|e| MetaError::Corrupt(format!("a group's record cannot be written: {e}")))
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, MetaError> {
        let corrupt = |why: &str| MetaError::Corrupt(format!("group record: {why}"));
        let (version, body) = bytes.split_first().ok_or_else(|| corrupt("empty"))?;
        match *version {
            VERSION => postcard::from_bytes(body).map_err(|e| corrupt(&e.to_string())),
            1 => {
                let old: V1 = postcard::from_bytes(body).map_err(|e| corrupt(&e.to_string()))?;
                Ok(Self {
                    live: old.live,
                    session_ms: old.session_ms,
                    empty_since_ms: old.empty_since_ms,
                    protocol_type: old.protocol_type,
                    consumer: None,
                })
            }
            _ => Err(corrupt(&format!("version {version} is not known"))),
        }
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
            consumer: Some(ConsumerLife {
                epoch: 3,
                assignment_epoch: 2,
                assignor: "uniform".into(),
            }),
        };
        let mut bytes = life.encode().unwrap();
        assert_eq!(GroupLife::decode(&bytes).unwrap(), life);
        bytes[0] = 9;
        assert!(GroupLife::decode(&bytes).is_err());
        assert!(GroupLife::decode(&[]).is_err());
    }
}

#[cfg(test)]
mod compatibility {
    use super::*;

    #[test]
    fn a_record_the_first_version_wrote_is_read_as_a_classic_group() {
        // Version byte 1, then postcard of (live, session_ms, empty_since_ms, protocol_type) as that version had it.
        #[derive(Serialize)]
        struct First {
            live: bool,
            session_ms: u32,
            empty_since_ms: Option<i64>,
            protocol_type: String,
        }
        let first = First {
            live: false,
            session_ms: 0,
            empty_since_ms: Some(42),
            protocol_type: "consumer".into(),
        };
        let bytes = postcard::to_extend(&first, vec![1]).unwrap();
        let life = GroupLife::decode(&bytes).unwrap();
        assert_eq!((life.empty_since_ms, life.consumer), (Some(42), None));
        assert_eq!(life.protocol_type, "consumer");
    }
}
