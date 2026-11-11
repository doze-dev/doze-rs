//! A redb database that runs on the [`doze_fs::fs::Fs`] seam, shared by every doze service.
//!
//! - [`Store`]: opens (or atomically makes) the database file, so a simulated disk that cuts the power
//!   under a service cuts it under its metadata too.
//! - [`record`]: a version byte, then postcard. The bytes are ours and not redb's, so a change of either
//!   library cannot change what is stored, and a record of an unknown version is refused, not misread.
//! - [`GroupCommit`]: one writer thread that applies every queued operation in a single transaction and
//!   flushes once, so durable writes at 10,000 a second cost a few hundred flushes (spike S6).
//!
//! Extracted from `kafka-meta` (A1); the group-commit writer is new.

mod backend;
mod error;
mod group;
pub mod record;
mod store;

pub use backend::FileBackend;
pub use error::StoreError;
pub use group::{Ack, GroupCommit, Op};
pub use store::Store;
