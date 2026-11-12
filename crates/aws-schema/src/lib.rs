//! Schema-driven zero-copy decode, validate and encode over the packed tables the `aws-model-*` crates generate.
//!
//! One decoder per protocol reads a request body into an arena of nodes that point back into the body, checking it
//! against the schema as it goes (types, required members, lengths, ranges, enums, patterns, unions). Typed views then
//! read the arena through generated `Key` constants. No serde, no `unsafe`; a decode with a reused arena allocates nothing.
//! From spike S1b (design C, 2026-11-09).

pub mod arena;
pub mod build;
pub mod cbor;
pub mod enc;
pub mod json;
pub mod meta;
pub mod patterns;
pub mod tables;
pub mod time;
pub mod view;
