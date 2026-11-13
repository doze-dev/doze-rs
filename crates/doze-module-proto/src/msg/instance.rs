//! What the host tells a module about an instance, and how the module describes running it.

use super::value::Node;
use minicbor::{Decode, Encode};
use std::collections::BTreeMap;

/// Where something listens: a Unix socket path, or a loopback TCP address (Windows backends, PLAN.md D13).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum Address {
    #[n(0)]
    Unix(#[n(0)] String),
    #[n(1)]
    Tcp(#[n(0)] String),
}

/// A listener the host set up for the instance.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Granted {
    /// Where clients connect (host:port).
    #[n(0)]
    pub addr: String,
    #[n(1)]
    pub url: String,
    #[n(2)]
    pub hosts: Vec<String>,
    /// Where the engine itself must listen for this listener.
    #[n(3)]
    pub backend: Address,
}

/// A dependency that is up and held for the instance.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Dep {
    #[n(0)]
    pub name: String,
    #[n(1)]
    pub engine: String,
    #[n(2)]
    pub backend: Address,
    #[n(3)]
    pub url: Option<String>,
}

/// A resolved set of executables.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Toolchain {
    #[n(0)]
    pub engine: String,
    #[n(1)]
    pub full: String,
    #[n(2)]
    pub bin_dir: String,
    /// Tools found elsewhere than `bin_dir`, by logical name.
    #[n(3)]
    #[cbor(default)]
    pub tools: BTreeMap<String, String>,
}

/// One instance, as every per-instance call receives it.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct Instance {
    #[n(0)]
    pub name: String,
    #[n(1)]
    pub engine: String,
    /// The declared version ("17", "16.14"); empty for a versionless engine.
    #[n(2)]
    pub version: String,
    #[n(3)]
    pub data_dir: String,
    #[n(4)]
    pub socket_dir: String,
    /// The evaluated, schema-checked configuration block (without the core's own arguments).
    #[n(5)]
    pub spec: Node,
    /// The instance's host name ("db.shop.doze"); empty when the host gives no names.
    #[n(6)]
    pub host: String,
    #[n(7)]
    #[cbor(default)]
    pub listeners: BTreeMap<String, Granted>,
    #[n(8)]
    #[cbor(default)]
    pub deps: Vec<Dep>,
    /// Filled in for calls that run the engine's own tools (admin, console, converge).
    #[n(9)]
    pub toolchain: Option<Toolchain>,
}

/// A listener the module wants (protocol 1's `Network`).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Listener {
    #[n(0)]
    pub name: String,
    /// The conventional port (0 for "main": the instance's own port).
    #[n(1)]
    pub port: u16,
    #[n(2)]
    pub http: bool,
    #[n(3)]
    #[cbor(default)]
    pub routes: Vec<String>,
    /// The engine cannot listen on a Unix socket (temporal's gRPC server, the ferret gateway, kafka): the host grants
    /// it a loopback TCP backend even where it grants others sockets (D13).
    #[n(4)]
    #[cbor(default)]
    pub tcp: bool,
}

/// The connection string doze surfaces for an instance.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Connection {
    /// The conventional variable name (DATABASE_URL).
    #[n(0)]
    pub env_var: String,
    #[n(1)]
    pub url: String,
    /// More variables a client of this engine needs (aws: AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION). The
    /// module says so; protocol 1's core guessed it from an `AWS_ENDPOINT_URL` prefix.
    #[n(2)]
    #[cbor(default)]
    pub env: BTreeMap<String, String>,
}

/// One structural object an instance manages (role, database, queue). `hash` is defined by the protocol: SHA-256 of the
/// canonical CBOR of the object's declared fields, first 12 hex digits (protocol.md row 15).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Object {
    #[n(0)]
    pub kind: String,
    #[n(1)]
    pub name: String,
    #[n(2)]
    pub hash: String,
}
