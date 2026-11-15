//! Where the registry keeps its records: opaque keys and values, written atomically. The broker's metadata database holds
//! them ([`MetaStorage`]), so the registry survives a restart with no topic to replay (Confluent keeps its state in a
//! `_schemas` topic; nothing on the wire observes the difference). [`MemoryStorage`] is for tests, and for a registry that
//! need not outlive its process.
//!
//! The keys are typed (`Key`) and written as a tag byte and the parts; the values are a version byte and postcard, as
//! the broker's own records are.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::format::SchemaType;

/// Why the store failed.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct StoreError(pub String);

/// The registry's records, kept somewhere durable.
pub trait Storage: Send + Sync + std::fmt::Debug {
    /// Every record.
    ///
    /// # Errors
    /// The store cannot be read.
    fn load(&self) -> Result<kafka_meta::Records, StoreError>;

    /// Removes `deletes` and writes `puts`, all or nothing.
    ///
    /// # Errors
    /// The write failed; nothing changed.
    fn apply(&self, puts: &[(Vec<u8>, Vec<u8>)], deletes: &[Vec<u8>]) -> Result<(), StoreError>;
}

/// The broker's metadata database.
#[derive(Debug, Clone)]
pub struct MetaStorage(pub Arc<kafka_meta::MetaStore>);

impl Storage for MetaStorage {
    fn load(&self) -> Result<kafka_meta::Records, StoreError> {
        self.0
            .registry_records()
            .map_err(|e| StoreError(e.to_string()))
    }

    fn apply(&self, puts: &[(Vec<u8>, Vec<u8>)], deletes: &[Vec<u8>]) -> Result<(), StoreError> {
        self.0
            .registry_apply(puts, deletes)
            .map_err(|e| StoreError(e.to_string()))
    }
}

/// Records in memory. Two registries opened over one show what a restart would find.
#[derive(Debug, Default, Clone)]
pub struct MemoryStorage(Arc<Mutex<BTreeMap<Vec<u8>, Vec<u8>>>>);

impl Storage for MemoryStorage {
    fn load(&self) -> Result<kafka_meta::Records, StoreError> {
        let m = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    fn apply(&self, puts: &[(Vec<u8>, Vec<u8>)], deletes: &[Vec<u8>]) -> Result<(), StoreError> {
        let mut m = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        for k in deletes {
            m.remove(k);
        }
        for (k, v) in puts {
            m.insert(k.clone(), v.clone());
        }
        Ok(())
    }
}

/// What a record is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Key {
    /// A schema, by id.
    Schema(u32),
    /// A version of a subject.
    Version(String, u32),
    /// A subject's compatibility level; the empty subject is the global one.
    Level(String),
    /// A subject's mode; the empty subject is the global one.
    Mode(String),
    /// The next id to give.
    NextId,
}

impl Key {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Self::Schema(id) => {
                out.push(b's');
                out.extend(id.to_be_bytes());
            }
            Self::Version(subject, v) => {
                out.push(b'v');
                out.extend(subject.as_bytes());
                out.push(0);
                out.extend(v.to_be_bytes());
            }
            Self::Level(subject) => {
                out.push(b'c');
                out.extend(subject.as_bytes());
            }
            Self::Mode(subject) => {
                out.push(b'm');
                out.extend(subject.as_bytes());
            }
            Self::NextId => out.push(b'n'),
        }
        out
    }

    /// The key `raw` is; `None` for one this version does not know.
    pub(crate) fn decode(raw: &[u8]) -> Option<Self> {
        let (&tag, rest) = raw.split_first()?;
        let text = |b: &[u8]| String::from_utf8(b.to_vec()).ok();
        let num = |b: &[u8]| <[u8; 4]>::try_from(b).ok().map(u32::from_be_bytes);
        match tag {
            b's' => num(rest).map(Self::Schema),
            b'v' => {
                let at = rest.len().checked_sub(5)?;
                let (subject, tail) = rest.split_at_checked(at)?;
                let (&zero, v) = tail.split_first()?;
                (zero == 0).then_some(())?;
                Some(Self::Version(text(subject)?, num(v)?))
            }
            b'c' => text(rest).map(Self::Level),
            b'm' => text(rest).map(Self::Mode),
            b'n' if rest.is_empty() => Some(Self::NextId),
            _ => None,
        }
    }
}

/// A reference, as a schema names another.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Reference {
    pub name: String,
    pub subject: String,
    pub version: i64,
}

/// A stored schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SchemaRecord {
    pub(crate) kind: SchemaType,
    pub(crate) text: String,
    pub(crate) refs: Vec<Reference>,
}

/// A stored version of a subject, with who registered it, when and from where.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VersionRecord {
    pub(crate) id: u32,
    pub(crate) deleted: bool,
    /// Unix milliseconds.
    pub(crate) at: i64,
    pub(crate) by: String,
    pub(crate) from: String,
    pub(crate) agent: String,
}

/// A level or a mode, and when it was set (Unix milliseconds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Setting {
    pub(crate) value: String,
    pub(crate) at: i64,
}

/// The version byte every value starts with.
const FORMAT: u8 = 1;

pub(crate) fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    let mut out = vec![FORMAT];
    // postcard cannot fail on these types: every field is a string, a number or a list of them.
    out.extend(postcard::to_allocvec(value).unwrap_or_default());
    out
}

pub(crate) fn decode<T: DeserializeOwned>(raw: &[u8]) -> Result<T, String> {
    match raw.split_first() {
        Some((&FORMAT, rest)) => postcard::from_bytes(rest).map_err(|e| e.to_string()),
        Some((v, _)) => Err(format!("record format {v} is not one this version reads")),
        None => Err("an empty record".into()),
    }
}

/// The writes of one operation, landed together.
#[derive(Debug, Default)]
pub(crate) struct Batch {
    pub(crate) puts: Vec<(Vec<u8>, Vec<u8>)>,
    pub(crate) deletes: Vec<Vec<u8>>,
}

impl Batch {
    pub(crate) fn put<T: Serialize>(&mut self, key: &Key, value: &T) {
        self.puts.push((key.encode(), encode(value)));
    }

    pub(crate) fn delete(&mut self, key: &Key) {
        self.deletes.push(key.encode());
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.puts.is_empty() && self.deletes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_reads_back_as_itself_and_strangers_are_none() {
        for k in [
            Key::Schema(7),
            Key::Version("orders-value".into(), 3),
            Key::Version(String::new(), 1),
            Key::Level(String::new()),
            Key::Level("a".into()),
            Key::Mode("m".into()),
            Key::NextId,
        ] {
            assert_eq!(Key::decode(&k.encode()), Some(k));
        }
        for raw in [
            &b""[..],
            b"x",
            b"s\0\0",
            b"v\0\0\0\0",
            b"v1\x01\0\0\0\x01",
            b"n1",
        ] {
            assert_eq!(Key::decode(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn a_value_of_another_format_or_none_is_refused() {
        let s = Setting {
            value: "FULL".into(),
            at: 5,
        };
        assert_eq!(decode::<Setting>(&encode(&s)).unwrap(), s);
        assert!(decode::<Setting>(&[]).is_err());
        assert!(decode::<Setting>(&[2, 0]).is_err());
        assert!(decode::<Setting>(&[1, 0xff]).is_err());
    }
}
