//! Admin data and console layout (protocol 1's Admin and Console, now independent capabilities), and the one-way
//! frames a module may send: log records and events.

use minicbor::{Decode, Encode};
use std::collections::BTreeMap;

/// A sub-resource of an instance (a queue, a database, a topic) with a one-line status.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Resource {
    #[n(0)]
    pub kind: String,
    #[n(1)]
    pub name: String,
    #[n(2)]
    pub status: String,
    #[n(3)]
    #[cbor(default)]
    pub info: BTreeMap<String, String>,
}

/// A data operation the module offers on a kind of resource.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Action {
    #[n(0)]
    pub id: String,
    #[n(1)]
    pub label: String,
    #[n(2)]
    pub kind: String,
    #[n(3)]
    pub destructive: bool,
    #[n(4)]
    pub input_hint: Option<String>,
}

/// How the host should draw the module's console: panels of data and a layout over them.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct ConsoleSpec {
    #[n(0)]
    pub panels: Vec<Panel>,
    #[n(1)]
    #[cbor(default)]
    pub tabs: Vec<Tab>,
    /// A web console the module serves, as a template over the instance's address ("http://{address}/_console").
    #[n(2)]
    pub web_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Panel {
    #[n(0)]
    pub id: String,
    #[n(1)]
    pub title: String,
    /// "table", "counters" or "stream".
    #[n(2)]
    pub kind: String,
    /// For a table: the resource kind it lists. For a stream: the table panel whose selection it follows.
    #[n(3)]
    pub source: String,
    #[n(4)]
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Column {
    #[n(0)]
    pub key: String,
    #[n(1)]
    pub title: String,
    /// How the host formats it: "", "count", "rate", "lag", "state", "time", "code".
    #[n(2)]
    pub role: String,
    #[n(3)]
    pub right: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Tab {
    #[n(0)]
    pub id: String,
    #[n(1)]
    pub title: String,
    #[n(2)]
    pub layout: Layout,
}

/// A row or column of blocks, or a block that draws a panel.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum Layout {
    #[n(0)]
    Row(#[n(0)] Vec<Layout>),
    #[n(1)]
    Col(#[n(0)] Vec<Layout>),
    #[n(2)]
    Panel(#[n(0)] String),
    #[n(3)]
    Note(#[n(0)] String),
}

/// `tail` arguments: stream rows of a stream panel.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct TailArgs {
    #[n(0)]
    pub panel: String,
    /// The selected row of the table the stream follows; empty for none.
    #[n(1)]
    pub selector: String,
}

/// One streamed row.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Row {
    #[n(0)]
    pub cells: BTreeMap<String, String>,
    /// "", "ok", "warn" or "bad".
    #[n(1)]
    pub tone: String,
}

/// A log record (one-way, module to host).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct LogRecord {
    /// 0 error, 1 warn, 2 info, 3 debug.
    #[n(0)]
    pub level: u8,
    #[n(1)]
    pub message: String,
    /// The instance it concerns, if one.
    #[n(2)]
    pub instance: Option<String>,
    #[n(3)]
    #[cbor(default)]
    pub fields: BTreeMap<String, String>,
}

/// An event (one-way, module to host): something the host should know without asking.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum Event {
    /// An instance's health changed (supervised modules).
    #[n(0)]
    Health {
        #[n(0)]
        instance: String,
        #[n(1)]
        healthy: bool,
        #[n(2)]
        detail: String,
    },
    /// Progress of a long call (a download), for the CLI and the app.
    #[n(1)]
    Progress {
        #[n(0)]
        for_call: u32,
        #[n(1)]
        done: u64,
        #[n(2)]
        total: u64,
        #[n(3)]
        what: String,
    },
}

/// `wire.accept` arguments: a client connection rides with this frame.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct WireAccept {
    #[n(0)]
    pub instance: String,
    #[n(1)]
    pub require_tls: bool,
    #[n(2)]
    pub local: bool,
    /// The host's TLS certificate and key (DER), when it terminates TLS for this listener.
    #[n(3)]
    pub tls: Option<(minicbor::bytes::ByteVec, minicbor::bytes::ByteVec)>,
}
