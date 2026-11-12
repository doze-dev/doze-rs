//! Which groups read which topics: the groups that have committed offsets on them (ListGroups, then OffsetFetch).
//! A group that reads a topic without ever committing does not appear, and the pages say "committed" and not "reading".

use std::collections::{BTreeMap, BTreeSet};

use kafka_wire::api::Response;

use crate::backend::{Backend, requests};

/// The groups of the broker and, for each topic, the groups with a committed offset on it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Readers {
    pub groups: usize,
    pub by_topic: BTreeMap<String, BTreeSet<String>>,
}

/// Asks the broker. At most 200 groups are looked into (50 to a request).
pub(super) async fn all(b: &Backend) -> Readers {
    let Ok(Response::ListGroups(list)) = b.call(requests::list_groups()).await else {
        return Readers::default();
    };
    let names: Vec<String> = list
        .groups
        .iter()
        .map(|g| g.group_id.0.to_string())
        .collect();
    let mut out = Readers {
        groups: names.len(),
        by_topic: BTreeMap::new(),
    };
    for chunk in names.chunks(50).take(4) {
        let Ok(Response::OffsetFetch(r)) = b.call(requests::committed_of(chunk)).await else {
            continue;
        };
        for g in r.groups.iter().filter(|g| g.error_code == 0) {
            for t in &g.topics {
                if t.partitions.iter().any(|p| p.committed_offset >= 0) {
                    out.by_topic
                        .entry(t.name.0.to_string())
                        .or_default()
                        .insert(g.group_id.0.to_string());
                }
            }
        }
    }
    out
}
