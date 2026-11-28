//! How long tables and indexes stay in their transitional states, and what the host hands each region's actor.

use std::sync::Arc;

use aws_peers::Peers;

/// How long a table or index stays in a transitional state (`CREATING`, `UPDATING`, `DELETING`), in milliseconds. AWS takes seconds
/// to minutes; the default here is none, so a waiter passes on its first probe, and a test that wants to see the state turns them up
/// (`doze-aws --dynamodb-create-ms` and its siblings; a host that passes none falls back to `DOZE_DDB_CREATE_MS`, `DOZE_DDB_UPDATE_MS`
/// and `DOZE_DDB_DELETE_MS`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Windows {
    /// `CREATING`, after `CreateTable` (and an index added by `UpdateTable`: `CREATING` on the index).
    pub create_ms: i64,
    /// `UPDATING`, after `UpdateTable`.
    pub update_ms: i64,
    /// `DELETING`, after `DeleteTable`.
    pub delete_ms: i64,
}

/// What the host hands each region's actor: the bus, and the transitional windows.
#[derive(Debug)]
pub(crate) struct Ext {
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) windows: Windows,
}

impl Windows {
    pub(crate) fn from_env() -> Self {
        let read = |name: &str| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
        };
        Self {
            create_ms: read("DOZE_DDB_CREATE_MS"),
            update_ms: read("DOZE_DDB_UPDATE_MS"),
            delete_ms: read("DOZE_DDB_DELETE_MS"),
        }
    }
}
