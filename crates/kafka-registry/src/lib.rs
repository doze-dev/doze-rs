//! A Confluent-compatible schema registry, served beside the broker (docs/port/phase10.md).
//!
//! - [`registry`]: subjects, versions, global ids, levels and modes, and the rules around them, written through to a
//!   [`store::Storage`] (the broker's metadata database in the binary).
//! - [`format`](mod@format): the seam a schema language plugs into; [`avro`], [`protobuf`] and [`jsonschema`] are the
//!   languages, listed in [`format::Formats::all`].
//! - [`api`]: the REST interface, request in, response out; [`server`] puts it on the network.
//! - [`wire`]: the bytes a Confluent serializer frames a record value with.

pub mod api;
pub mod avro;
pub mod error;
pub mod format;
pub mod json;
pub mod jsonschema;
pub mod protobuf;
pub mod registry;
pub mod server;
pub mod store;
pub mod wire;

pub use api::{Api, Options};
pub use error::RegistryError;
pub use registry::{Registry, SchemaInput};
