//! The Consumers tab of a topic: the groups with offsets committed on it (ListGroups, OffsetFetch), each described
//! (DescribeGroups), with the log end of every partition (ListOffsets) so that the lag is the same arithmetic as the Groups pages.
//!
//! [`view`] is the pure half (the fixtures run it too); [`consumers`] asks the broker. A group that reads a topic without ever
//! committing does not appear (the tab's count says the same: groups with a committed offset).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::group::{Loaded, block};
use super::group_read::{self as read, Listed};
use super::groups::phase_of;
use super::meta;
use crate::backend::Backend;
use crate::model::{ConsumersView, TopicHead, TopicReader};

/// The most groups a tab lists (and so the most looked into).
pub const MOST_READERS: usize = 25;

/// The tab from what was read: `groups` are the groups that committed on the topic, loaded; `ends` are its log ends.
#[must_use]
pub fn view(
    head: TopicHead,
    total: usize,
    groups: &[(Listed, Loaded)],
    ends: &HashMap<(String, i32), i64>,
) -> ConsumersView {
    let name = head.name.clone();
    let parts = head.parts.len();
    let mut readers: Vec<TopicReader> = groups
        .iter()
        .map(|(listed, loaded)| {
            let (phase, protocol) = phase_of(listed, loaded);
            let members = loaded.described.as_ref().map_or(0, |d| d.members.len());
            let holding = loaded.described.as_ref().map_or(0, |d| {
                d.members
                    .iter()
                    .filter(|m| m.holds.iter().any(|p| p.topic == name))
                    .count()
            });
            let block = block(&name, loaded, Some(parts), ends, None);
            TopicReader {
                id: listed.id.clone(),
                protocol,
                phase,
                members,
                holding,
                last_commit_ms: loaded.detail.as_ref().and_then(|d| {
                    d.offsets
                        .iter()
                        .filter(|o| o.topic == name)
                        .map(|o| o.committed_ms)
                        .max()
                }),
                block,
            }
        })
        .collect();
    // The ones furthest behind first, then by name: what needs a look comes up.
    readers.sort_by(|a, b| {
        b.block
            .behind
            .cmp(&a.block.behind)
            .then_with(|| a.id.cmp(&b.id))
    });
    ConsumersView {
        head,
        readers,
        total,
    }
}

/// The tab of the topic `name`, or `None` if there is no such topic (or the console may not describe it).
pub async fn consumers(b: &Backend, name: &str) -> Option<ConsumersView> {
    let meta = meta::one(b, name).await?;
    let head = super::topic::head_of(b, &meta).await;
    let committed: BTreeSet<String> = super::readers::all(b)
        .await
        .by_topic
        .remove(name)
        .unwrap_or_default();
    let total = committed.len();
    let shown: Vec<String> = committed.into_iter().take(MOST_READERS).collect();
    let mut described = read::describe(b, &shown).await;
    let mut commits = read::commits(b, &shown).await;
    let mut groups = Vec::new();
    for id in &shown {
        let loaded = Loaded {
            described: described.remove(id),
            detail: b.group_detail(id).await,
            commits: commits.remove(id).unwrap_or_default(),
            events: None,
        };
        let protocol = loaded
            .described
            .as_ref()
            .map_or(crate::model::GroupProtocol::Classic, |d| d.protocol);
        groups.push((
            Listed {
                id: id.clone(),
                protocol,
            },
            loaded,
        ));
    }
    let asked: BTreeMap<String, BTreeSet<i32>> = BTreeMap::from([(
        name.to_owned(),
        (0..i32::try_from(head.parts.len()).unwrap_or(i32::MAX)).collect(),
    )]);
    let ends = read::log_bounds(b, &asked, -1).await;
    Some(view(head, total, &groups, &ends))
}
