//! doze.hcl (core plan phase H4): doze's schema over doze-hcl. It reads a stack's files the way
//! Go's `internal/config` does and gives the engine its typed stack, or the diagnostics Go gives,
//! at Go's positions.
//!
//! Built so far: the typed model ([`model`]) and the oracle corpus in `testdata/oracle` (see
//! `testdata/tools/README.md`). The loader, validation, the shape pass and the conversion into
//! `doze_engine::stack` are next.

pub mod model;

pub use model::{Config, Instance, Kind, Ref, Spec};
