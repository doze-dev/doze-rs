//! What the service keeps about a table (not its items): the schema, the indexes, the stream, the time-to-live setting, the tags.
//! One record per table, loaded when the service opens and written when it changes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A table's or index's lifecycle state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Status {
    Creating,
    Active,
    Updating,
    Deleting,
}

impl Status {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Creating => "CREATING",
            Self::Active => "ACTIVE",
            Self::Updating => "UPDATING",
            Self::Deleting => "DELETING",
        }
    }
}

/// A key attribute and its scalar type (`S`, `N` or `B`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Key {
    pub(crate) name: String,
    pub(crate) ty: char,
}

/// How much of an item an index holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Projection {
    All,
    KeysOnly,
    Include(Vec<String>),
}

/// A secondary index.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Index {
    /// The number its entries are filed under.
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) hash: Key,
    pub(crate) range: Option<Key>,
    pub(crate) projection: Projection,
    /// Provisioned read and write units, for a global index on a provisioned table.
    pub(crate) provisioned: Option<(i64, i64)>,
    pub(crate) status: Status,
    pub(crate) status_until_ms: i64,
}

/// How a table is billed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Billing {
    pub(crate) pay_per_request: bool,
    pub(crate) read_units: i64,
    pub(crate) write_units: i64,
    /// When the table last became pay-per-request (its creation, if it was born so).
    pub(crate) pay_per_request_since_ms: i64,
}

/// A table's stream.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Stream {
    pub(crate) id: [u8; 16],
    /// `2026-11-12T10:20:30.456`, the time the stream was made: part of its ARN.
    pub(crate) label: String,
    /// `KEYS_ONLY`, `NEW_IMAGE`, `OLD_IMAGE` or `NEW_AND_OLD_IMAGES`.
    pub(crate) view: String,
    pub(crate) enabled: bool,
    /// The sequence number of the last record appended.
    pub(crate) last_seq: u64,
}

/// The time-to-live setting.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Ttl {
    pub(crate) attribute: String,
    pub(crate) enabled: bool,
    /// When it last changed: AWS refuses another change within the hour.
    pub(crate) changed_ms: i64,
}

/// Point-in-time recovery.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Pitr {
    pub(crate) enabled: bool,
    pub(crate) days: i64,
    pub(crate) enabled_ms: i64,
}

/// Everything about a table but its items.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Table {
    pub(crate) id: [u8; 16],
    pub(crate) name: String,
    pub(crate) created_ms: i64,
    pub(crate) status: Status,
    pub(crate) status_until_ms: i64,
    pub(crate) hash: Key,
    pub(crate) range: Option<Key>,
    pub(crate) billing: Billing,
    pub(crate) gsis: Vec<Index>,
    pub(crate) lsis: Vec<Index>,
    pub(crate) next_index: u32,
    pub(crate) stream: Option<Stream>,
    pub(crate) ttl: Ttl,
    pub(crate) protect: bool,
    pub(crate) infrequent: bool,
    /// `KMS` when the table asked for encryption under a key of its own.
    pub(crate) sse: Option<(String, Option<String>)>,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) pitr: Pitr,
    pub(crate) on_demand: Option<(i64, i64)>,
    pub(crate) warm: Option<(i64, i64)>,
}

impl Table {
    /// The table's id as the text AWS shows.
    pub(crate) fn id_text(&self) -> String {
        uuid_text(&self.id)
    }

    /// The names of the key attributes of the table and of every index, with their types, by name.
    pub(crate) fn attribute_definitions(&self) -> BTreeMap<String, char> {
        let table_keys = std::iter::once(&self.hash).chain(self.range.as_ref());
        let index_keys = self
            .gsis
            .iter()
            .chain(&self.lsis)
            .flat_map(|i| std::iter::once(&i.hash).chain(i.range.as_ref()));
        table_keys
            .chain(index_keys)
            .map(|k| (k.name.clone(), k.ty))
            .collect()
    }

    /// An index by name.
    pub(crate) fn index(&self, name: &str) -> Option<&Index> {
        self.gsis.iter().chain(&self.lsis).find(|i| i.name == name)
    }
}

/// `8-4-4-4-12` lower-case hex.
pub(crate) fn uuid_text(id: &[u8; 16]) -> String {
    let hex = aws_core::hex::encode(id);
    let part = |a: usize, b: usize| hex.get(a..b).unwrap_or_default();
    format!(
        "{}-{}-{}-{}-{}",
        part(0, 8),
        part(8, 12),
        part(12, 16),
        part(16, 20),
        part(20, 32)
    )
}
