//! A module's console as the daemon relays it (Go `ConsoleSpec` and its parts, `ConsoleFrame`).

use super::frame::ActionView;
use crate::json::is_zero_i64;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The cell a table row carries when its resource was made while the stack ran and is not in the config.
pub const CELL_RUNTIME: &str = "_runtime";

/// A module's description of its console (Go `ConsoleSpec`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsoleSpec {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub front: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub web_url: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub panels: Vec<ConsolePanel>,
    /// How the panels are laid out; none means the default layout.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<ConsoleTab>,
    /// Blocks added to the generic screen.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extend: Vec<ConsoleExtension>,
}

/// One tab of a console (Go `ConsoleTab`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsoleTab {
    pub id: String,
    pub title: String,
    pub body: ConsoleNode,
}

/// A block put on the generic overview after the block named by `after` (Go `ConsoleExtension`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsoleExtension {
    pub after: String,
    pub node: ConsoleNode,
}

/// A container (`row`, `col`) or a block (`table`, `stat`, `detail`, `stream`, `actions`, `note`, `chart`)
/// (Go `ConsoleNode`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsoleNode {
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub weight: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub panel: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub from: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ConsoleNode>,
}

/// One block of a console: a table, a row of counters, or a stream (Go `ConsolePanel`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsolePanel {
    pub id: String,
    pub title: String,
    /// `table`, `counters` or `stream`.
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<ConsoleColumn>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub select: String,
}

/// One column of a table, one field of a stream row (Go `ConsoleColumn`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsoleColumn {
    pub key: String,
    pub title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub align: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub role: String,
}

/// One row: its cells by column key, and a tone (Go `ConsoleRow`). Go writes a row with no cells map as
/// `"cells":null`, which is `None` here, so it re-encodes the same.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsoleRow {
    pub cells: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tone: String,
}

impl ConsoleRow {
    /// The cell under column `key`.
    pub fn cell(&self, key: &str) -> Option<&str> {
        self.cells.as_ref()?.get(key).map(String::as_str)
    }
}

/// One message of the `console` topic (Go `ConsoleFrame`). The first carries the spec and the actions; then a table
/// or counters panel is sent whole each time it is read (`rows`), and a stream panel one row at a time (`row`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsoleFrame {
    pub instance: String,
    /// Whether the instance is awake: a console never wakes what it looks at.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<ConsoleSpec>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ActionView>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub panel: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<ConsoleRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<ConsoleRow>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}
