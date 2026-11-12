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

/// How the host draws the module's console and what the module adds to the app's own pages (the Go SDK's
/// `engine.ConsoleSpec`, kept whole: the app renders `extend` blocks on the instance overview). The module gives data
/// and layout; the host draws, so every module's console looks like the rest of the app.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct ConsoleSpec {
    /// The data the console draws: tables, counters, streams.
    #[n(0)]
    pub panels: Vec<Panel>,
    /// Tabs laying the panels out; with none the host lays them out itself (tables side by side, streams below).
    #[n(1)]
    #[cbor(default)]
    pub tabs: Vec<Tab>,
    /// A web console the module serves, as a template over the instance's address ("http://{address}/_console"), or a
    /// named listener's (`{address:ui}`: temporal's Web UI is its own listener).
    #[n(2)]
    pub web_url: Option<String>,
    /// The tab the console opens on: "console", "web", or none for the host's default for the kind.
    #[n(3)]
    pub front: Option<String>,
    /// Blocks the module adds to the app's generic instance overview, each after one of its cards.
    #[n(4)]
    #[cbor(default)]
    pub extend: Vec<Extension>,
}

/// A data panel.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Panel {
    #[n(0)]
    pub id: String,
    #[n(1)]
    pub title: String,
    /// "table", "counters" or "stream".
    #[n(2)]
    pub kind: String,
    /// For a table or counters: the resource kind it lists.
    #[n(3)]
    pub source: String,
    #[n(4)]
    pub columns: Vec<Column>,
    /// For a stream: the table panel whose selected row picks what to stream.
    #[n(5)]
    pub select: Option<String>,
}

/// A column of a table, a field of a stream row.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Column {
    /// "name", "status", or a key of the resource's info (or the row's cells).
    #[n(0)]
    pub key: String,
    #[n(1)]
    pub title: String,
    /// How the host formats and colours it: "", "count", "rate", "lag", "state", "time", "code". A "rate" column's
    /// key names an info value that is a running total; the host shows how fast it grows.
    #[n(2)]
    pub role: String,
    /// "left" or "right".
    #[n(3)]
    pub align: String,
}

/// One tab of the console.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Tab {
    #[n(0)]
    pub id: String,
    #[n(1)]
    pub title: String,
    #[n(2)]
    pub body: Node,
}

/// A block on the app's overview of the instance, after the card named.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Extension {
    /// "connect", "awake", "vitals" or "recent".
    #[n(0)]
    pub after: String,
    #[n(1)]
    pub node: Node,
}

/// A layout node. Containers: "row" and "col" (children laid out by weight). Blocks: "table", "stat", "detail",
/// "stream", "actions", "note", "chart". The kind is a string on purpose: a host that meets a kind it does not know
/// draws a placeholder card and the rest of the layout as usual, so a newer module never breaks an older app.
#[derive(Debug, Clone, PartialEq, Eq, Default, Encode, Decode)]
pub struct Node {
    #[n(0)]
    pub kind: String,
    #[n(1)]
    pub id: String,
    #[n(2)]
    pub title: String,
    #[n(3)]
    pub weight: u32,
    /// The panel a block draws.
    #[n(4)]
    pub panel: String,
    /// A "detail" or "stream" follows the row selected in this table block.
    #[n(5)]
    pub from: String,
    /// A note's text.
    #[n(6)]
    pub text: String,
    /// An "actions" block's actions; none means all.
    #[n(7)]
    #[cbor(default)]
    pub actions: Vec<String>,
    #[n(8)]
    #[cbor(default)]
    pub children: Vec<Node>,
}

/// `tail` arguments: stream rows of a stream panel.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct TailArgs {
    #[n(0)]
    pub panel: String,
    /// The selected row of the table the stream follows; empty for none.
    #[n(1)]
    pub selector: String,
    #[n(2)]
    pub instance: super::instance::Instance,
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
