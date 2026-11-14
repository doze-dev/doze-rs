//! A fixture daemon's script: a file of JSON lines, one entry each, in the order they play.
//!
//! ```text
//! {"status":{"ok":true,"stack":"shop","instances":[…]}}    what `status` answers
//! {"hold":true}                                              keep subscriptions open after the frames have played
//! {"frame":{"topic":"instances","snapshot":true,…}}          a frame played to each subscriber that asked for its topic
//! {"wait_ms":50}                                             a pause between frames
//! {"log":{"instance":"api","line":"listening",…}}            a line an instance printed, for the `logs` op
//! {"admin_result":"purged 3 messages"}                       what `admin` answers
//! {"fail":{"key":"wake api","error":"boom","code":"timeout"}} an op ("op" or "op target") that fails
//! ```
//!
//! The scripts of the Go fixture hubs (shop, crash, modules, larder, …) were captured from Go's own
//! `internal/control/fixture` and are in `testdata/go/scripts`.

use crate::{Frame, LogFrame, Response, codec};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

/// One line of a script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entry {
    Status(Response),
    Hold(bool),
    Frame(Box<Frame>),
    WaitMs(u64),
    Log(LogFrame),
    AdminResult(String),
    Fail(Fail),
}

/// An op that fails: `key` is `"op target"` or `"op"` (`"admin <action>"` for a data action).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fail {
    pub key: String,
    pub error: String,
    /// The fault kind sent with it (`not_found`, …); empty for plain `failed`.
    pub code: String,
}

/// One step a subscriber is played.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Frame(Box<Frame>),
    Wait(Duration),
}

/// A whole script.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Script {
    /// What `status` answers (and what verbs change, as a daemon would).
    pub status: Response,
    /// Keep a subscription open after the script has played, until it is cancelled or the daemon hangs up; without
    /// it the stream ends, which is what lets a test of a "follow" finish.
    pub hold: bool,
    pub steps: Vec<Step>,
    /// What each instance has printed.
    pub logs: Vec<LogFrame>,
    pub admin_result: String,
    pub fail: Vec<Fail>,
}

/// A script line that could not be read.
#[derive(Debug, thiserror::Error)]
#[error("{source_name}:{line}: {message}")]
pub struct ScriptError {
    pub source_name: String,
    pub line: usize,
    pub message: String,
}

impl Script {
    /// Reads a script from its text; `name` is for errors.
    pub fn parse(name: &str, text: &str) -> Result<Script, ScriptError> {
        let mut s = Script::default();
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            let entry: Entry = serde_json::from_str(line).map_err(|e| ScriptError {
                source_name: name.to_owned(),
                line: i + 1,
                message: e.to_string(),
            })?;
            s.add(entry);
        }
        Ok(s)
    }

    /// Reads a script file.
    pub fn load(path: &Path) -> Result<Script, ScriptError> {
        let name = path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|e| ScriptError {
            source_name: name.clone(),
            line: 0,
            message: e.to_string(),
        })?;
        Script::parse(&name, &text)
    }

    /// Adds one entry, as if it were the next line.
    pub fn add(&mut self, entry: Entry) {
        match entry {
            Entry::Status(r) => self.status = r,
            Entry::Hold(h) => self.hold = h,
            Entry::Frame(f) => self.steps.push(Step::Frame(f)),
            Entry::WaitMs(ms) => self.steps.push(Step::Wait(Duration::from_millis(ms))),
            Entry::Log(l) => self.logs.push(l),
            Entry::AdminResult(r) => self.admin_result = r,
            Entry::Fail(f) => self.fail.push(f),
        }
    }

    /// The script as a file, one entry per line.
    pub fn to_text(&self) -> String {
        let mut entries = vec![Entry::Status(self.status.clone())];
        entries.extend(self.hold.then_some(Entry::Hold(true)));
        entries.extend(
            (!self.admin_result.is_empty()).then(|| Entry::AdminResult(self.admin_result.clone())),
        );
        entries.extend(self.fail.iter().cloned().map(Entry::Fail));
        entries.extend(self.steps.iter().map(|s| match s {
            Step::Frame(f) => Entry::Frame(f.clone()),
            Step::Wait(d) => Entry::WaitMs(u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        }));
        entries.extend(self.logs.iter().cloned().map(Entry::Log));
        entries
            .iter()
            .filter_map(|e| codec::encode(e).ok())
            .map(|l| String::from_utf8_lossy(&l).into_owned())
            .collect()
    }

    /// The failure scripted for `op` on `target`, if any.
    pub(crate) fn failure(&self, op: &str, target: &str) -> Option<&Fail> {
        let exact = format!("{op} {target}");
        self.fail
            .iter()
            .find(|f| f.key == exact)
            .or_else(|| self.fail.iter().find(|f| f.key == op))
    }
}
