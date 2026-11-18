//! A client quota as it is kept on disk: who it is for (Kafka's `ClientQuotaEntity`) and the values set for it. A version
//! byte, then postcard, as for ACLs and credentials.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::MetaError;

const VERSION: u8 = 1;

/// A name in a quota entity: one user, client id or address, or the default for every one that has no quota of its own
/// (Kafka writes the default as a null name, and its tools as `<default>`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum QuotaName {
    /// The default of its type.
    Default,
    /// This name, as the client wrote it.
    Named(String),
}

impl QuotaName {
    /// The name, or `None` for the default: the form Kafka puts on the wire.
    #[must_use]
    pub fn as_wire(&self) -> Option<&str> {
        match self {
            Self::Default => None,
            Self::Named(name) => Some(name),
        }
    }

    /// The name a wire entity carries (`None` is the default).
    #[must_use]
    pub fn from_wire(name: Option<&str>) -> Self {
        name.map_or(Self::Default, |n| Self::Named(n.to_owned()))
    }
}

/// Who a quota is for: a user, a client id, both, or an address, each by name or as the default. Kafka allows the user and
/// client id together and the address alone (`ClientQuotaControlManager.configKeysForEntityType`); this type holds whatever
/// the broker accepted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct QuotaEntity {
    /// `user`: a principal's name (the part after `User:`).
    pub user: Option<QuotaName>,
    /// `client-id`: the id a client gives in each request's header.
    pub client_id: Option<QuotaName>,
    /// `ip`: the address connections come from.
    pub ip: Option<QuotaName>,
}

impl QuotaEntity {
    /// The key of the table: the entity as postcard, which is one byte string per entity and the same every time.
    pub(crate) fn key(&self) -> Result<Vec<u8>, MetaError> {
        postcard::to_extend(self, vec![VERSION])
            .map_err(|e| MetaError::Corrupt(format!("a quota entity cannot be written: {e}")))
    }

    pub(crate) fn from_key(bytes: &[u8]) -> Result<Self, MetaError> {
        decode(bytes, "quota entity")
    }
}

/// The values set for one entity: quota name (`producer_byte_rate`, ...) to its value, as Kafka keeps them (doubles).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuotaValues(pub BTreeMap<String, f64>);

impl QuotaValues {
    pub(crate) fn encode(&self) -> Result<Vec<u8>, MetaError> {
        postcard::to_extend(self, vec![VERSION])
            .map_err(|e| MetaError::Corrupt(format!("quota values cannot be written: {e}")))
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, MetaError> {
        decode(bytes, "quota values")
    }
}

fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8], what: &str) -> Result<T, MetaError> {
    let corrupt = |why: &str| MetaError::Corrupt(format!("{what}: {why}"));
    let (version, body) = bytes.split_first().ok_or_else(|| corrupt("empty"))?;
    if *version != VERSION {
        return Err(corrupt(&format!("version {version} is not known")));
    }
    postcard::from_bytes(body).map_err(|e| corrupt(&e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entity_and_its_values_read_back_as_written() {
        let entity = QuotaEntity {
            user: Some(QuotaName::Named("alice".into())),
            client_id: Some(QuotaName::Default),
            ip: None,
        };
        assert_eq!(
            QuotaEntity::from_key(&entity.key().unwrap()).unwrap(),
            entity
        );
        let values = QuotaValues(BTreeMap::from([
            ("producer_byte_rate".to_owned(), 1024.0),
            ("request_percentage".to_owned(), 12.5),
        ]));
        assert_eq!(
            QuotaValues::decode(&values.encode().unwrap()).unwrap(),
            values
        );
    }

    #[test]
    fn a_record_of_another_version_is_refused_not_misread() {
        assert!(QuotaValues::decode(&[9, 0]).is_err());
        assert!(QuotaEntity::from_key(&[]).is_err());
    }

    #[test]
    fn the_wire_form_of_a_name_is_null_for_the_default() {
        assert_eq!(QuotaName::from_wire(None), QuotaName::Default);
        assert_eq!(QuotaName::Named("c1".into()).as_wire(), Some("c1"));
        assert_eq!(QuotaName::Default.as_wire(), None);
    }
}
