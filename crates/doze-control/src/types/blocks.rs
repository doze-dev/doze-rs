//! The console's working blocks as the daemon relays them (Rust daemon, additive; module protocol minor 2): part of one
//! value, a tree folder's children, a form's fields, a check, a time chart's readings. Every field
//! is left out when empty, so a frame without them is the frame Go wrote, and a client that does not know them reads
//! the rest as before.

use super::console::ConsoleRow;
use crate::json::{is_false, is_zero_u64};
use crate::time::Timestamp;
use base64ct::{Base64, Encoding};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How often the daemon reads an awake instance's tables for its time charts, whether or not anyone looks.
pub const READINGS_EVERY_SECS: u64 = 15;

/// How many readings it keeps per instance (15 minutes of them).
pub const READINGS_KEPT: usize = 60;

/// One typed field of an action's form (module `Field`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FieldView {
    pub id: String,
    pub label: String,
    /// `text`, `secret`, `number`, `choice`, `bool` or `pairs`; an unknown kind is a text box.
    pub kind: String,
    #[serde(skip_serializing_if = "is_false")]
    pub required: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub default: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub help: String,
}

/// Part of one value (the `value` op's answer).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ValueView {
    /// `json`, `text`, `bytes` or `pairs`; an unknown one is shown as bytes.
    pub media: String,
    /// The part, as UTF-8 text, or base64 when `base64` is set (bytes, or text that is not UTF-8).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub body: String,
    #[serde(skip_serializing_if = "is_false")]
    pub base64: bool,
    /// Key–value pairs in order (headers, a hash's fields).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pairs: Vec<PairView>,
    /// The whole value's size in bytes.
    pub size: u64,
    /// Where this part starts.
    #[serde(skip_serializing_if = "is_zero_u64")]
    pub offset: u64,
    /// More follows: ask again from `offset` + the part's length.
    #[serde(skip_serializing_if = "is_false")]
    pub more: bool,
}

/// One key and its value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PairView {
    pub key: String,
    pub value: String,
}

impl ValueView {
    /// Holds `part` as text when it is UTF-8 and the media is not bytes, as base64 otherwise.
    pub fn set_body(&mut self, part: &[u8]) {
        match std::str::from_utf8(part) {
            Ok(s) if self.media != "bytes" => {
                s.clone_into(&mut self.body);
                self.base64 = false;
            }
            _ => {
                self.body = Base64::encode_string(part);
                self.base64 = true;
            }
        }
    }

    /// The part's bytes; `None` when its base64 does not read.
    pub fn part(&self) -> Option<Vec<u8>> {
        if self.base64 {
            Base64::decode_vec(&self.body).ok()
        } else {
            Some(self.body.as_bytes().to_vec())
        }
    }
}

/// One page of a tree folder's children (the `children` op's answer).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChildrenView {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<TreeNodeView>,
    /// The cursor of the next page; empty at the end.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub next: String,
}

/// A folder or a leaf; a leaf opens in the value viewer, asked with the row `{"id": id}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TreeNodeView {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "is_false")]
    pub leaf: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub info: BTreeMap<String, String>,
}

/// A failing check, as the map and Needs you show it beside a crash (`InstanceView::checks`). Only `bad` checks are
/// carried there; a `warn` stays in the module's console.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CheckView {
    /// What was checked.
    pub name: String,
    /// `bad`.
    pub status: String,
    /// The result in plain words.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// The id of the action that mends it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fix: String,
}

/// One past reading of a table panel, for its time charts (`ConsoleFrame::history`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsoleReading {
    pub at: Timestamp,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<ConsoleRow>,
}
