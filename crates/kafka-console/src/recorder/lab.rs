//! What the Lab asks of the recorder: the requests a rule touched, and the connections an app has open.

use super::Recorder;
use super::view::wire;
use crate::model::live::WireRow;

impl Recorder {
    /// The newest requests the Lab rule `rule` touched, newest first, at most `limit`; and whether older ones may have left the
    /// ring (it is bounded, so a rule that touched many requests is not listed in full).
    #[must_use]
    pub fn lab_hits(&self, rule: u32, limit: usize) -> (Vec<WireRow>, bool) {
        let inner = self.lock();
        let all: Vec<WireRow> = inner
            .ring
            .iter()
            .rev()
            .filter(|r| r.lab.as_ref().is_some_and(|l| l.rule == rule))
            .map(wire)
            .collect();
        let more = all.len() > limit;
        (all.into_iter().take(limit).collect(), more)
    }

    /// For each app with a connection open that has said who it is: its name and how many connections, by name.
    #[must_use]
    pub fn open_clients(&self) -> Vec<(String, usize)> {
        let inner = self.lock();
        let mut by = std::collections::BTreeMap::<String, usize>::new();
        for (_, c) in inner.conns.iter() {
            if !c.who.client.is_empty() {
                *by.entry(c.who.client.to_string()).or_default() += 1;
            }
        }
        by.into_iter().collect()
    }

    /// The highest connection number among the connections open now: a connection accepted later has a higher one.
    #[must_use]
    pub fn highest_open_conn(&self) -> Option<u64> {
        self.lock().conns.iter().map(|(id, _)| *id).max()
    }
}
