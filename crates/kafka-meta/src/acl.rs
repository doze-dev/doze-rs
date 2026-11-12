//! An access control entry as it is kept on disk: the seven fields a client sent, with the codes as numbers, so
//! that what a client stored is what it reads back. A version byte, then postcard, as for topics.

use serde::{Deserialize, Serialize};

use crate::MetaError;

const VERSION: u8 = 1;

/// One ACL. Two records with the same seven fields are the same ACL.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AclRecord {
    /// The resource type, as on the wire.
    pub resource_type: i8,
    /// The resource name, or the prefix of names.
    pub name: String,
    /// The pattern type, as on the wire.
    pub pattern_type: i8,
    /// The principal, `Type:name`.
    pub principal: String,
    /// `*`, or a client address.
    pub host: String,
    /// The operation, as on the wire.
    pub operation: i8,
    /// The permission type, as on the wire.
    pub permission: i8,
}

impl AclRecord {
    /// What the table is keyed by: the seven fields and nothing else, so that a repeat lands on the same row
    /// whatever version wrote it.
    pub(crate) fn key(&self) -> Result<Vec<u8>, MetaError> {
        postcard::to_extend(self, Vec::new())
            .map_err(|e| MetaError::Corrupt(format!("an ACL cannot be written: {e}")))
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, MetaError> {
        postcard::to_extend(self, vec![VERSION])
            .map_err(|e| MetaError::Corrupt(format!("an ACL cannot be written: {e}")))
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, MetaError> {
        let corrupt = |why: &str| MetaError::Corrupt(format!("ACL record: {why}"));
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

    fn record() -> AclRecord {
        AclRecord {
            resource_type: 2,
            name: "orders".into(),
            pattern_type: 3,
            principal: "User:alice".into(),
            host: "*".into(),
            operation: 8,
            permission: 3,
        }
    }

    #[test]
    fn a_record_is_read_back_whole_and_an_unknown_version_is_refused() {
        let r = record();
        let bytes = r.encode().unwrap();
        assert_eq!(bytes.first(), Some(&VERSION));
        assert_eq!(AclRecord::decode(&bytes).unwrap(), r);
        let mut other = bytes.clone();
        other[0] = 9;
        assert!(AclRecord::decode(&other).is_err());
        assert!(AclRecord::decode(&[]).is_err());
        assert!(AclRecord::decode(&bytes[..bytes.len() - 1]).is_err());
    }

    #[test]
    fn the_key_tells_apart_every_field_and_ignores_none_of_them() {
        let r = record();
        let mut seen = std::collections::HashSet::from([r.key().unwrap()]);
        let changed = [
            AclRecord {
                resource_type: 3,
                ..r.clone()
            },
            AclRecord {
                name: "order".into(),
                ..r.clone()
            },
            AclRecord {
                pattern_type: 4,
                ..r.clone()
            },
            AclRecord {
                principal: "User:bob".into(),
                ..r.clone()
            },
            AclRecord {
                host: "1.2.3.4".into(),
                ..r.clone()
            },
            AclRecord {
                operation: 3,
                ..r.clone()
            },
            AclRecord {
                permission: 2,
                ..r.clone()
            },
            // Strings that run together differently are different keys.
            AclRecord {
                name: "ab".into(),
                principal: "c".into(),
                ..r.clone()
            },
            AclRecord {
                name: "a".into(),
                principal: "bc".into(),
                ..r.clone()
            },
        ];
        for c in changed {
            assert!(
                seen.insert(c.key().unwrap()),
                "{c:?} has a key that is taken"
            );
        }
        assert_eq!(r.key().unwrap(), r.clone().key().unwrap());
    }
}
