//! Sans-IO Kafka wire format: framing, versions and record batches. No I/O lives here.
//!
//! Nothing in this crate reads a socket or a file. A caller hands it bytes and gets values back,
//! which is what lets every parser be fuzzed and unit-tested without a network.

pub mod api;
pub mod api_versions;
pub mod batch;
pub mod error;
pub mod frame;
pub mod header;
pub mod ids;
pub mod incoming;
pub mod profile;
pub mod response;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;

pub use error::WireError;
/// The generated message types and error codes, re-exported so that no other crate names `kafka-protocol`: its
/// version is chosen in one place.
pub use kafka_protocol::{ResponseError, messages};
