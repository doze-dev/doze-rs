//! The first exchange: the module says who it is and everything static about it (protocol.md rows 1 to 3); the host
//! answers with the agreed version and the limits it enforces.

use super::console::{Action, ConsoleSpec};
use super::value::Value;
use minicbor::{Decode, Encode};

/// A protocol version on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct WireVersion {
    #[n(0)]
    pub major: u16,
    #[n(1)]
    pub minor: u16,
}

/// Capabilities, as bits. An older host ignores bits it does not know.
pub mod caps {
    pub const CONVERGE: u64 = 1 << 0;
    pub const OBJECTS: u64 = 1 << 1;
    pub const PRUNE: u64 = 1 << 2;
    pub const ATTRIBUTES: u64 = 1 << 3;
    pub const BACKEND_URL: u64 = 1 << 4;
    pub const TEMPLATE: u64 = 1 << 5;
    pub const RESTART: u64 = 1 << 6;
    pub const HEALTH: u64 = 1 << 7;
    pub const ADMIN: u64 = 1 << 8;
    pub const CONSOLE: u64 = 1 << 9;
    pub const WIRE: u64 = 1 << 10;
    pub const EVENTS: u64 = 1 << 11;
}

/// `hello`, module to host, the first frame on the channel.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct Hello {
    #[n(0)]
    pub version: WireVersion,
    /// The module's own name and release, e.g. "postgres", "0.3.0".
    #[n(1)]
    pub module: String,
    #[n(2)]
    pub release: String,
    #[n(3)]
    pub manifest: Manifest,
}

/// The host's answer to `hello`.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Welcome {
    /// The version both use from here on.
    #[n(0)]
    pub version: WireVersion,
    /// Calls the host will have in flight at once at most.
    #[n(1)]
    pub max_calls: u32,
    /// Log records per second the host keeps; the module's SDK drops (and counts) the rest.
    #[n(2)]
    pub log_rate: u32,
}

/// Everything static about a module, sent once (protocol 1 asked for most of it call by call).
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct Manifest {
    /// The block keyword, e.g. "postgres".
    #[n(0)]
    pub engine: String,
    #[n(1)]
    pub category: String,
    #[n(2)]
    pub title: String,
    #[n(3)]
    pub tagline: String,
    /// Selectable engine versions (majors), newest last; empty for a versionless engine.
    #[n(4)]
    pub versions: Vec<String>,
    /// How many dots make a version exact rather than a major ("16.14" with 1, "8.1.8" with 2).
    #[n(5)]
    pub exact_dots: u8,
    #[n(6)]
    pub default_port: u16,
    /// How long a cold boot may take, when longer than the host's default.
    #[n(7)]
    pub boot_budget_ms: Option<u32>,
    /// Supervised (always on, never reaped when idle) rather than lazy.
    #[n(8)]
    pub supervised: bool,
    /// The backend's socket file name in the instance's socket directory, `{port}` filled in (Unix); on Windows the
    /// host gives the backend a loopback port instead (PLAN.md D13).
    #[n(9)]
    pub backend_socket: Option<String>,
    #[n(10)]
    pub caps: u64,
    #[n(11)]
    pub schema: Schema,
    /// A complete example block, used by the registry and checked by the module's acceptance tests.
    #[n(12)]
    pub example: String,
    #[n(13)]
    pub console: Option<ConsoleSpec>,
    #[n(14)]
    #[cbor(default)]
    pub actions: Vec<Action>,
    /// The targets this module supports (PLAN.md D12), e.g. "x86_64-pc-windows-msvc".
    #[n(15)]
    pub targets: Vec<String>,
}

/// The shape of a module's configuration block: the core validates against it before the module sees anything.
#[derive(Debug, Clone, PartialEq, Default, Encode, Decode)]
pub struct Schema {
    #[n(0)]
    pub attrs: Vec<Attr>,
    #[n(1)]
    pub blocks: Vec<BlockSchema>,
}

/// One argument.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct Attr {
    #[n(0)]
    pub name: String,
    #[n(1)]
    pub ty: Type,
    #[n(2)]
    pub required: bool,
    #[n(3)]
    pub default: Option<Value>,
    #[n(4)]
    pub doc: String,
    /// The engine majors the argument applies to (inclusive since, exclusive until); the core enforces them.
    #[n(5)]
    pub since: Option<String>,
    #[n(6)]
    pub until: Option<String>,
}

/// A nested block type.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct BlockSchema {
    #[n(0)]
    pub name: String,
    /// What the labels mean ("name"); empty for an unlabeled block.
    #[n(1)]
    pub labels: Vec<String>,
    /// May appear more than once (labels must then be unique).
    #[n(2)]
    pub repeated: bool,
    #[n(3)]
    pub doc: String,
    #[n(4)]
    pub schema: Schema,
}

/// An argument's type.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub enum Type {
    #[n(0)]
    String,
    #[n(1)]
    Number,
    #[n(2)]
    Bool,
    #[n(3)]
    List(#[n(0)] Box<Type>),
    #[n(4)]
    Map(#[n(0)] Box<Type>),
    #[n(5)]
    Any,
}
