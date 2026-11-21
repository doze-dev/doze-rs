//! The records a region keeps beside its tables, in one table of the file (`extras`), each under a key that says what it is:
//! `backup/<id>` an on-demand backup (its items are in the backup space), `export/<id>` and `import/<id>` the tasks, `policy/<arn>`
//! the resource policy of a table or stream, `kinesis/<table>` a table's Kinesis streaming destinations, and `restore/<table>` where
//! a restored table came from. All are small; they are
//! loaded when the region opens, like the table records.

use serde::{Deserialize, Serialize};

use crate::meta::Table;

pub(crate) const VERSION: u8 = 1;

/// One record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum Extra {
    Backup(Box<Backup>),
    /// An export or an import: what its describe answers, as JSON text, and what a repeated client token is held against.
    Task(Task),
    Policy(Policy),
    Kinesis(Vec<Destination>),
    /// Where a restored table came from, for its describe's `RestoreSummary`.
    Restored(Restored),
}

/// A table made by a restore.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Restored {
    pub(crate) backup_arn: Option<String>,
    pub(crate) source_arn: String,
    pub(crate) at_ms: i64,
}

pub(crate) fn restored_key(table: &str) -> String {
    format!("restore/{table}")
}

/// An on-demand backup: the table as it was, and how much it holds. Its items are filed under `id` in the backup space.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Backup {
    pub(crate) id: [u8; 16],
    pub(crate) arn: String,
    pub(crate) name: String,
    pub(crate) created_ms: i64,
    /// `USER`, or `SYSTEM` for the one made when a table with point-in-time recovery is deleted.
    pub(crate) system: bool,
    pub(crate) table: Table,
    pub(crate) table_arn: String,
    pub(crate) items: u64,
    pub(crate) bytes: u64,
}

/// An export or import task.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Task {
    pub(crate) arn: String,
    /// The table's ARN (the export's source, the import's target).
    pub(crate) table_arn: String,
    pub(crate) started_ms: i64,
    /// The client token and the request it came with, as AWS holds a repeat against it for 8 hours.
    pub(crate) token: Option<(String, String)>,
    /// The description, as JSON text.
    pub(crate) shown: String,
}

/// A resource policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Policy {
    pub(crate) text: String,
    pub(crate) revision: String,
}

/// A Kinesis data stream a table's changes are sent to.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Destination {
    pub(crate) stream_arn: String,
    /// `ENABLING`, `ACTIVE`, `DISABLING`, `DISABLED`, `ENABLE_FAILED` or `UPDATING`.
    pub(crate) status: String,
    pub(crate) description: Option<String>,
    /// `MILLISECOND` or `MICROSECOND`, when the caller set one (records are in milliseconds otherwise).
    #[serde(default)]
    pub(crate) precision: Option<String>,
}

pub(crate) fn backup_key(id: &str) -> String {
    format!("backup/{id}")
}

pub(crate) fn export_key(arn: &str) -> String {
    format!("export/{arn}")
}

pub(crate) fn import_key(arn: &str) -> String {
    format!("import/{arn}")
}

pub(crate) fn policy_key(arn: &str) -> String {
    format!("policy/{arn}")
}

pub(crate) fn kinesis_key(table: &str) -> String {
    format!("kinesis/{table}")
}

/// `<14 digits of milliseconds>-<8 hex>`: the last part of a backup, export or import ARN, as AWS writes it.
pub(crate) fn task_id(now_ms: i64) -> Option<String> {
    let tail = aws_seal::random_bytes::<4>()?;
    Some(format!(
        "{:014}-{}",
        now_ms.max(0),
        aws_core::hex::encode(&tail)
    ))
}
