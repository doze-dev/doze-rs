//! Taking over the data a Go doze-kafka module left in an instance's folder, once, before the Rust broker first opens it.
//!
//! Go's partition logs are already what the Rust broker reads: `<topic>-<n>/` folders of `%020d.log` segments holding
//! Kafka RecordBatch v2 bytes as received, with 16-byte `.index` entries and the same `log-start` file. Only the
//! metadata differs: Go kept it in `meta.db` (bbolt, JSON values), the Rust broker in `meta.redb`. So the take-over is:
//!
//! 1. read `meta.db` (read-only, with doze-bolt): topics with their ids, partitions and configs; committed offsets; the
//!    next producer id;
//! 2. in each partition folder, Go's `transactions` note becomes the Rust broker's `transactional` (so a log that held
//!    transactional batches is still scanned for them), and Go's `cleaner-offset` goes (compaction re-cleans);
//! 3. Go's `clean-shutdown` and `unvalidated` go, so the broker checks the logs' last batches as after a crash;
//! 4. `meta.redb` is built in a side folder and renamed into place, so a stop half way leaves no half-made database;
//! 5. `meta.db` is kept, renamed `meta.db.go`, beside the data it described.
//!
//! What is not carried (said in the log, and kept in `meta.db.go`): ACLs, transactional ids' state, the schema
//! registry's subjects and member-less groups' liveness (their offsets are kept, dated now, so the usual retention
//! starts again from the take-over).

use std::collections::BTreeMap;
use std::path::Path;

use doze_bolt::Db;
use kafka_meta::{MetaStore, OffsetCommit, StoredOffset, Topic};
use kafka_storage::fs::RealFs;
use kafka_wire::ids::{GroupId, PartitionCount, PartitionId, TopicId, TopicName};
use serde::Deserialize;

/// What became of a take-over: counts, and what could not be carried.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TookOver {
    pub(crate) topics: usize,
    pub(crate) offsets: usize,
    pub(crate) left: Vec<String>,
}

#[derive(Deserialize)]
struct GoTopic {
    id: [u8; 16],
    partitions: i32,
    #[serde(default)]
    config: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct GoOffset {
    offset: i64,
    #[serde(default)]
    metadata: Option<String>,
    #[serde(default)]
    at: Option<i64>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn rename_note(dir: &Path) -> std::io::Result<()> {
    let go = dir.join("transactions");
    if go.is_file() {
        std::fs::write(dir.join("transactional"), b"yes\n")?;
        std::fs::remove_file(go)?;
    }
    match std::fs::remove_file(dir.join("cleaner-offset")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Whether `data` holds a Go broker's data that has not been taken over.
pub(crate) fn pending(data: &Path) -> bool {
    data.join("meta.db").is_file()
        && !std::fs::read_dir(data).is_ok_and(|d| {
            d.flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("meta.redb"))
        })
}

/// Takes over the Go data in `data`, if there is any; `None` when there was nothing to do.
///
/// # Errors
/// `meta.db` cannot be read, or the new database cannot be made; the folder is then as Go left it, but for the notes of
/// step 2, which the next attempt makes again.
pub(crate) fn take_over(data: &Path) -> Result<Option<TookOver>, String> {
    if !pending(data) {
        return Ok(None);
    }
    let go = Db::open(&data.join("meta.db")).map_err(|e| format!("reading meta.db: {e}"))?;
    let mut done = TookOver::default();
    let topics = go_topics(&go, &mut done.left)?;
    for e in std::fs::read_dir(data)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        if e.path().is_dir() {
            rename_note(&e.path()).map_err(|err| format!("{}: {err}", e.path().display()))?;
        }
    }
    for marker in ["clean-shutdown", "unvalidated"] {
        let _ = std::fs::remove_file(data.join(marker));
    }

    let side = data.join(".meta-from-go");
    let _ = std::fs::remove_dir_all(&side);
    std::fs::create_dir_all(&side).map_err(|e| e.to_string())?;
    {
        let meta = MetaStore::open(&RealFs, &side).map_err(|e| e.to_string())?;
        for t in topics {
            if meta.import_topic(t).map_err(|e| e.to_string())? {
                done.topics += 1;
            }
        }
        done.offsets = offsets(&go, &meta, &mut done.left)?;
        let next = go.values(&[b"meta"]).map_err(|e| e.to_string())?;
        if let Some((_, v)) = next.iter().find(|(k, _)| k == b"producer_id")
            && let Ok(be) = <[u8; 8]>::try_from(v.as_slice())
        {
            meta.raise_producer_ids(i64::from_be_bytes(be))
                .map_err(|e| e.to_string())?;
        }
    }
    uncarried(&go, &mut done.left)?;
    for e in std::fs::read_dir(&side)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        std::fs::rename(e.path(), data.join(e.file_name())).map_err(|e| e.to_string())?;
    }
    std::fs::rename(data.join("meta.db"), data.join("meta.db.go")).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir(&side);
    Ok(Some(done))
}

/// Go's topics: name, id, partitions and configs.
fn go_topics(go: &Db, left: &mut Vec<String>) -> Result<Vec<Topic>, String> {
    let mut topics = Vec::new();
    for (name, value) in go.values(&[b"topics"]).map_err(|e| e.to_string())? {
        let name = String::from_utf8_lossy(&name).into_owned();
        let t: GoTopic =
            serde_json::from_slice(&value).map_err(|e| format!("topic {name}: {e}"))?;
        let (Ok(topic), Some(count)) = (TopicName::parse(&name), PartitionCount::new(t.partitions))
        else {
            left.push(format!("topic {name:?}: not a valid topic"));
            continue;
        };
        topics.push(Topic::new(
            topic,
            TopicId::from_bytes(t.id),
            count,
            t.config,
        ));
    }
    Ok(topics)
}

/// What Go kept that is not carried over: ACLs, transactional ids, schema registry subjects, groups' liveness.
fn uncarried(go: &Db, left: &mut Vec<String>) -> Result<(), String> {
    for (bucket, what) in [
        (&b"acls"[..], "ACLs"),
        (b"txns", "transactional ids' state"),
        (b"registry", "schema registry subjects"),
    ] {
        if !go.items(&[bucket]).map_err(|e| e.to_string())?.is_empty() {
            left.push(format!("{what} (kept in meta.db.go)"));
        }
    }
    let lives = go.values(&[b"meta"]).map_err(|e| e.to_string())?;
    if lives.iter().any(|(k, _)| k.starts_with(b"empty_since\0")) {
        left.push("member-less groups' liveness (their offsets are kept)".into());
    }
    Ok(())
}

/// Says what a take-over did in the instance's log; a failed one stops the start (the Go data is left as it was).
///
/// # Errors
/// The take-over failed.
pub(crate) fn say<S>(
    cx: &doze_module::Cx<S>,
    outcome: Result<Option<TookOver>, String>,
) -> doze_module::Result<()> {
    use doze_module::proto::msg::error::{Error, ErrorKind};
    match outcome {
        Ok(None) => Ok(()),
        Ok(Some(t)) => {
            cx.log(
                2,
                format!(
                    "took over the Go doze-kafka data: {} topics, {} committed offsets (Go's meta.db kept as meta.db.go)",
                    t.topics, t.offsets
                ),
            );
            for what in t.left {
                cx.log(1, format!("not carried from the Go data: {what}"));
            }
            Ok(())
        }
        Err(why) => Err(Error::new(
            ErrorKind::Internal,
            format!("taking over the Go doze-kafka data: {why}"),
        )),
    }
}

/// Go's committed offsets, group by group: key `group \0 topic \0 partition (u32, big-endian)`.
fn offsets(go: &Db, meta: &MetaStore, left: &mut Vec<String>) -> Result<usize, String> {
    let mut by_group: BTreeMap<String, Vec<OffsetCommit>> = BTreeMap::new();
    for (key, value) in go.values(&[b"offsets"]).map_err(|e| e.to_string())? {
        let mut parts = key.splitn(3, |b| *b == 0);
        let (Some(group), Some(topic), Some(p)) = (parts.next(), parts.next(), parts.next()) else {
            left.push("an offset with a key Go never wrote".into());
            continue;
        };
        let partition = <[u8; 4]>::try_from(p)
            .ok()
            .and_then(|b| i32::try_from(u32::from_be_bytes(b)).ok())
            .and_then(PartitionId::new);
        let topic = TopicName::parse(&String::from_utf8_lossy(topic)).ok();
        let o: Option<GoOffset> = serde_json::from_slice(&value).ok();
        let (Some(partition), Some(topic), Some(o)) = (partition, topic, o) else {
            left.push("an offset Go wrote that cannot be read".into());
            continue;
        };
        by_group
            .entry(String::from_utf8_lossy(group).into_owned())
            .or_default()
            .push(OffsetCommit {
                topic,
                partition,
                offset: StoredOffset {
                    offset: o.offset,
                    leader_epoch: -1,
                    metadata: o.metadata.unwrap_or_default(),
                    committed_ms: o.at.filter(|at| *at > 0).unwrap_or_else(now_ms),
                },
            });
    }
    let mut stored = 0;
    for (group, commits) in by_group {
        let Ok(id) = GroupId::parse(&group) else {
            left.push(format!("group {group:?}: not a valid group id"));
            continue;
        };
        let outcomes = meta
            .commit_offsets(&id, &commits)
            .map_err(|e| e.to_string())?;
        stored += outcomes
            .iter()
            .filter(|o| **o == kafka_meta::Committed::Stored)
            .count();
    }
    Ok(stored)
}

#[cfg(test)]
mod tests;
