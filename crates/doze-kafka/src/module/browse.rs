//! The console's tree and value viewer (protocol minor 2): the topics, each opening on its partitions, each partition
//! on its newest messages (a page of [`PAGE`], newest first, paging back through older offsets), and the consumer
//! groups; a message opens whole in the value viewer with its headers beside it, a group as its offsets and lag.
//!
//! A node's id says where it is: `topics` and `groups` at the top, `t/<topic>`, `t/<topic>/<partition>`,
//! `t/<topic>/<partition>/<offset>` and `g/<group>`. Kafka allows no `/` in a topic name, but a topic is written with
//! `%` and `/` escaped all the same, so an id always reads back as what it was made from; a group, which may hold
//! anything, is the rest of its id.

use std::collections::BTreeMap;

use doze_module::caps::{Tree, Values};
use doze_module::proto::msg::blocks::{Children, TreeNode, ValueAnswer, media};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::{Cx, Result};

use super::client::browse::Record;
use super::client::{Client, TopicInfo};
use super::{Kafka, KafkaSpec, admin};

/// The tree's panel.
pub(crate) const PANEL: &str = "browse";
/// The messages one page of a partition holds.
pub(crate) const PAGE: i64 = 50;
/// How many windows of [`PAGE`] offsets a page looks back through for its messages (a compacted partition has gaps).
const LOOK_BACK: usize = 20;
/// The longest key shown beside a message, in characters.
const KEY_SHOWN: usize = 60;

/// Where a node of the tree is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum At {
    Top,
    Topics,
    Groups,
    Topic(String),
    Partition(String, i32),
    Message(String, i32, i64),
    Group(String),
}

/// A topic name as it is written in an id.
fn escape(topic: &str) -> String {
    topic.replace('%', "%25").replace('/', "%2F")
}

fn unescape(topic: &str) -> String {
    topic.replace("%2F", "/").replace("%25", "%")
}

impl At {
    /// The node's id.
    pub(crate) fn id(&self) -> String {
        match self {
            At::Top => String::new(),
            At::Topics => "topics".into(),
            At::Groups => "groups".into(),
            At::Topic(t) => format!("t/{}", escape(t)),
            At::Partition(t, p) => format!("t/{}/{p}", escape(t)),
            At::Message(t, p, o) => format!("t/{}/{p}/{o}", escape(t)),
            At::Group(g) => format!("g/{g}"),
        }
    }

    /// Where an id says; `None` for an id this tree never made.
    pub(crate) fn parse(id: &str) -> Option<At> {
        match id {
            "" => return Some(At::Top),
            "topics" => return Some(At::Topics),
            "groups" => return Some(At::Groups),
            _ => {}
        }
        if let Some(g) = id.strip_prefix("g/") {
            return (!g.is_empty()).then(|| At::Group(g.into()));
        }
        let parts: Vec<&str> = id.strip_prefix("t/")?.split('/').collect();
        let topic = unescape(parts.first().filter(|t| !t.is_empty())?);
        let number = |i: usize| parts.get(i).and_then(|n| n.parse::<i64>().ok());
        let partition = || {
            number(1)
                .and_then(|p| i32::try_from(p).ok())
                .filter(|p| *p >= 0)
        };
        match parts.len() {
            1 => Some(At::Topic(topic)),
            2 => Some(At::Partition(topic, partition()?)),
            3 => Some(At::Message(
                topic,
                partition()?,
                number(2).filter(|o| *o >= 0)?,
            )),
            _ => None,
        }
    }
}

/// Whether a name is what the person typed to filter by: a part of it, in any case.
pub(crate) fn matches(name: &str, filter: &str) -> bool {
    filter.is_empty() || name.to_lowercase().contains(&filter.trim().to_lowercase())
}

/// What a value is: JSON if it is an object or an array that parses, text if it is UTF-8, bytes otherwise.
pub(crate) fn media_of(value: &[u8]) -> &'static str {
    let Ok(text) = std::str::from_utf8(value) else {
        return media::BYTES;
    };
    let t = text.trim_start();
    if (t.starts_with('{') || t.starts_with('['))
        && serde_json::from_str::<serde_json::Value>(text).is_ok()
    {
        media::JSON
    } else {
        media::TEXT
    }
}

/// A header's or a key's bytes as words: the text if it is UTF-8, otherwise hex (`0x89504e47`).
pub(crate) fn shown(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(t) => t.to_string(),
        Err(_) => {
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            format!("0x{hex}")
        }
    }
}

/// The offsets a page reads below `before` in a partition that keeps offsets from `start`: from where, and the cursor
/// of the page after it (older offsets), if any are left.
pub(crate) fn window(start: i64, before: i64, size: i64) -> (i64, Option<i64>) {
    if before <= start {
        return (before, None);
    }
    let from = before.saturating_sub(size.max(1)).max(start);
    (from, (from > start).then_some(from))
}

fn folder(at: &At, label: &str, info: BTreeMap<String, String>) -> TreeNode {
    TreeNode {
        id: at.id(),
        label: label.into(),
        leaf: false,
        info,
    }
}

fn info(kv: &[(&str, String)]) -> BTreeMap<String, String> {
    kv.iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

/// A message as a leaf: its offset, its size and its key.
fn leaf(topic: &str, partition: i32, r: &Record) -> TreeNode {
    let mut i = info(&[("bytes", r.size.to_string())]);
    if let Some(k) = &r.key {
        i.insert("key".into(), shown(k).chars().take(KEY_SHOWN).collect());
    }
    TreeNode {
        leaf: true,
        ..folder(
            &At::Message(topic.into(), partition, r.offset),
            &format!("offset {}", r.offset),
            i,
        )
    }
}

/// The topics and the groups whose names match, as nodes.
async fn listed(c: &mut Client, filter: &str, topics: bool, groups: bool) -> Result<Vec<TreeNode>> {
    let mut out = Vec::new();
    for r in admin::resources(c).await? {
        if !matches(&r.name, filter) {
            continue;
        }
        let get = |k: &str| r.info.get(k).cloned().unwrap_or_default();
        if r.kind == "topic" && topics {
            let i = info(&[("partitions", get("partitions")), ("messages", get("high"))]);
            out.push(folder(&At::Topic(r.name.clone()), &r.name, i));
        } else if r.kind == "group" && groups {
            let i = info(&[("lag", get("lag")), ("members", get("members"))]);
            out.push(TreeNode {
                leaf: true,
                ..folder(&At::Group(r.name.clone()), &r.name, i)
            });
        }
    }
    Ok(out)
}

/// A topic as Metadata has it; not found if there is none of that name.
async fn topic(c: &mut Client, name: &str) -> Result<TopicInfo> {
    c.topics()
        .await?
        .remove(name)
        .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("there is no topic {name}")))
}

/// Where a partition's kept offsets start and end (the next to be written).
async fn bounds(c: &mut Client, topic: &str, partition: i32) -> Result<(i64, i64)> {
    let one = [TopicInfo {
        name: topic.into(),
        partitions: vec![partition],
    }];
    let at = (topic.to_string(), partition);
    let end = c.offsets(&one, -1).await?.get(&at).copied().unwrap_or(0);
    let start = c.offsets(&one, -2).await?.get(&at).copied().unwrap_or(0);
    Ok((start.min(end), end))
}

/// A page of a partition's messages, newest first, below the offset `cursor` names (the end for none).
async fn messages(
    c: &mut Client,
    topic: &str,
    partition: i32,
    cursor: Option<&str>,
) -> Result<Children> {
    let (start, end) = bounds(c, topic, partition).await?;
    let mut before = cursor
        .and_then(|c| c.parse::<i64>().ok())
        .map_or(end, |b| b.clamp(start, end));
    let mut got: Vec<Record> = Vec::new();
    let mut next = None;
    for _ in 0..LOOK_BACK {
        let want = PAGE - i64::try_from(got.len()).unwrap_or(PAGE);
        let (from, older) = window(start, before, want);
        let mut page = c.read(topic, partition, from, before, false).await?;
        page.reverse();
        got.extend(page);
        next = older;
        if older.is_none() || i64::try_from(got.len()).unwrap_or(PAGE) >= PAGE {
            break;
        }
        before = from;
    }
    Ok(Children {
        nodes: got.iter().map(|r| leaf(topic, partition, r)).collect(),
        next: next.map(|n| n.to_string()),
    })
}

/// The children of a node; a filter narrows the topics and groups by name (from the top, every one of them).
async fn children(c: &mut Client, at: &At, filter: &str, cursor: Option<&str>) -> Result<Children> {
    let all = match at {
        At::Top if filter.trim().is_empty() => vec![
            folder(&At::Topics, "Topics", BTreeMap::new()),
            folder(&At::Groups, "Consumer groups", BTreeMap::new()),
        ],
        At::Top => listed(c, filter, true, true).await?,
        At::Topics => listed(c, filter, true, false).await?,
        At::Groups => listed(c, filter, false, true).await?,
        At::Topic(name) => {
            let t = topic(c, name).await?;
            let ends = c.offsets(std::slice::from_ref(&t), -1).await?;
            t.partitions
                .iter()
                .map(|p| {
                    let n = ends.get(&(name.clone(), *p)).copied().unwrap_or(0);
                    let at = At::Partition(name.clone(), *p);
                    folder(
                        &at,
                        &format!("partition {p}"),
                        info(&[("messages", n.to_string())]),
                    )
                })
                .collect()
        }
        At::Partition(t, p) => return messages(c, t, *p, cursor).await,
        At::Message(..) | At::Group(_) => Vec::new(),
    };
    Ok(Children::page(all, cursor, 200))
}

/// A message whole: its value as what it is, its headers as pairs.
async fn message(
    c: &mut Client,
    at: (&str, i32, i64),
    offset: u64,
    max: u32,
) -> Result<ValueAnswer> {
    let (topic, partition, at) = at;
    let found = c
        .read(topic, partition, at, at.saturating_add(1), true)
        .await?;
    let Some(r) = found.into_iter().find(|r| r.offset == at) else {
        return Err(Error::new(
            ErrorKind::NotFound,
            format!("{topic} partition {partition} no longer holds offset {at}"),
        ));
    };
    let value = r.value.unwrap_or_default();
    let mut answer = ValueAnswer::slice(media_of(&value), &value, offset, max);
    answer.pairs = r
        .headers
        .iter()
        .map(|(k, v)| (shown(k), v.as_deref().map(shown).unwrap_or_default()))
        .collect();
    Ok(answer)
}

/// A group's committed offsets against the ends of its partitions, as JSON.
async fn group(c: &mut Client, name: &str) -> Result<Vec<u8>> {
    let Some(g) = c.groups().await?.into_iter().find(|g| g.name == name) else {
        return Err(Error::new(
            ErrorKind::NotFound,
            format!("there is no consumer group {name}"),
        ));
    };
    let committed = c.committed(name).await?;
    let mut by_topic: BTreeMap<String, Vec<i32>> = BTreeMap::new();
    for (t, p) in committed.keys() {
        by_topic.entry(t.clone()).or_default().push(*p);
    }
    let topics: Vec<TopicInfo> = by_topic
        .into_iter()
        .map(|(name, partitions)| TopicInfo { name, partitions })
        .collect();
    let ends = c.offsets(&topics, -1).await?;
    let mut total = 0_i64;
    let partitions: Vec<serde_json::Value> = committed
        .iter()
        .map(|(at, done)| {
            let end = ends.get(at).copied().unwrap_or(*done);
            let lag = (end - done).max(0);
            total += lag;
            serde_json::json!({"topic": at.0, "partition": at.1, "committed": done, "end": end, "lag": lag})
        })
        .collect();
    let doc = serde_json::json!({
        "group": g.name, "state": g.state, "members": g.members, "lag": total, "partitions": partitions,
    });
    serde_json::to_vec_pretty(&doc).map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))
}

impl Tree for Kafka {
    async fn children(
        &self,
        cx: &Cx<KafkaSpec>,
        panel: &str,
        parent: &str,
        filter: &str,
        cursor: Option<&str>,
    ) -> Result<Children> {
        let Some(at) = At::parse(parent).filter(|_| panel == PANEL) else {
            return Ok(Children::default());
        };
        children(
            &mut Client::connect(&cx.instance).await?,
            &at,
            filter,
            cursor,
        )
        .await
    }
}

impl Values for Kafka {
    async fn value(
        &self,
        cx: &Cx<KafkaSpec>,
        panel: &str,
        row: &BTreeMap<String, String>,
        offset: u64,
        max: u32,
    ) -> Result<ValueAnswer> {
        let id = row.get("id").map(String::as_str).unwrap_or_default();
        let unknown = || {
            Error::new(
                ErrorKind::NotFound,
                format!("the {panel} panel has no value {id:?}"),
            )
        };
        let at = At::parse(id)
            .filter(|_| panel == PANEL)
            .ok_or_else(unknown)?;
        let mut c = Client::connect(&cx.instance).await?;
        match at {
            At::Message(t, p, o) => message(&mut c, (&t, p, o), offset, max).await,
            At::Group(g) => Ok(ValueAnswer::slice(
                media::JSON,
                &group(&mut c, &g).await?,
                offset,
                max,
            )),
            _ => Err(unknown()),
        }
    }
}

#[cfg(test)]
mod tests;
