//! Which topic names may not live beside which.
//!
//! Two rules. Kafka's own: `a.b` and `a_b` would share metric names, so they may not both exist. Ours: names
//! that differ only in letter case would share a directory on a case-insensitive file system, so the same
//! data directory behaves the same wherever it is opened.

use std::collections::{HashMap, HashSet};
use std::fmt;

use kafka_wire::ids::TopicName;

/// Why a name cannot live beside one that exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collision {
    /// The name that was refused.
    pub name: TopicName,
    /// The topic it cannot live beside.
    pub existing: TopicName,
    /// Whether they differ only in letter case; otherwise only in `.` against `_`.
    pub case_only: bool,
}

impl fmt::Display for Collision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let how = if self.case_only {
            "letter case"
        } else {
            "'.' against '_'"
        };
        write!(
            f,
            "topic '{}' collides with existing topic '{}': they differ only in {how}",
            self.name, self.existing
        )
    }
}

/// The names under each key the two rules compare, so a new name is checked against the few that share its
/// key and not against every topic.
#[derive(Debug, Default)]
pub(crate) struct NameIndex {
    by_case: HashMap<String, Vec<TopicName>>,
    by_metric: HashMap<String, Vec<TopicName>>,
}

fn case_key(name: &TopicName) -> String {
    name.as_str().to_ascii_lowercase()
}

fn metric_key(name: &TopicName) -> String {
    name.as_str().replace('.', "_")
}

impl NameIndex {
    pub(crate) fn add(&mut self, name: &TopicName) {
        self.by_case
            .entry(case_key(name))
            .or_default()
            .push(name.clone());
        self.by_metric
            .entry(metric_key(name))
            .or_default()
            .push(name.clone());
    }

    pub(crate) fn remove(&mut self, name: &TopicName) {
        for (map, key) in [
            (&mut self.by_case, case_key(name)),
            (&mut self.by_metric, metric_key(name)),
        ] {
            if let Some(names) = map.get_mut(&key) {
                names.retain(|n| n != name);
                if names.is_empty() {
                    map.remove(&key);
                }
            }
        }
    }

    /// The topic `name` may not live beside, if there is one. Topics in `beside` (created earlier by the
    /// same request) are let off the `.` against `_` rule, and only that one: Kafka checks each name of a
    /// request against the topics that existed before it, so `a.b` and `a_b` sent together are both created.
    pub(crate) fn collision(
        &self,
        name: &TopicName,
        beside: &HashSet<TopicName>,
    ) -> Option<Collision> {
        let case = self.by_case.get(&case_key(name)).into_iter().flatten();
        if let Some(existing) = case.into_iter().find(|e| *e != name) {
            return Some(Collision {
                name: name.clone(),
                existing: existing.clone(),
                case_only: true,
            });
        }
        // Of several topics a name collides with, Kafka names the one created last.
        let metric = self.by_metric.get(&metric_key(name)).into_iter().flatten();
        let existing = metric.rev().find(|e| *e != name && !beside.contains(*e))?;
        Some(Collision {
            name: name.clone(),
            existing: existing.clone(),
            case_only: false,
        })
    }
}
