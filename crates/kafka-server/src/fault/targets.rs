//! What a request names that a rule can match on: its topics and partitions, and its group. Typed matches over the requests that
//! carry them; every other API names none.

use kafka_wire::Uuid;
use kafka_wire::api::Request;
use kafka_wire::messages::TopicName;

/// Looks a topic's name up by its id (a request from Fetch v13 and Produce v13 names a topic only by id).
pub type Names<'a> = &'a (dyn Fn(Uuid) -> Option<String> + Sync);

/// The name a request entry stands for: its own, or the one its id stands for.
fn named(name: &TopicName, id: Uuid, names: Names<'_>) -> Option<String> {
    if !name.0.is_empty() {
        return Some(name.0.to_string());
    }
    if id.is_nil() { None } else { names(id) }
}

/// Calls `f(topic, partition)` for every partition a request names (or `None` for a topic named without partitions), and stops at
/// the first `true`.
pub(super) fn any_target(
    request: &Request,
    names: Names<'_>,
    f: &mut dyn FnMut(&str, Option<i32>) -> bool,
) -> bool {
    match request {
        Request::Produce(r) => r.topic_data.iter().any(|t| {
            let ps = t.partition_data.iter().map(|p| p.index);
            topic(named(&t.name, t.topic_id, names), ps, f)
        }),
        Request::Fetch(r) => r.topics.iter().any(|t| {
            let ps = t.partitions.iter().map(|p| p.partition);
            topic(named(&t.topic, t.topic_id, names), ps, f)
        }),
        Request::ListOffsets(r) => r.topics.iter().any(|t| {
            let ps = t.partitions.iter().map(|p| p.partition_index);
            topic(Some(t.name.0.to_string()), ps, f)
        }),
        Request::OffsetCommit(r) => r.topics.iter().any(|t| {
            let ps = t.partitions.iter().map(|p| p.partition_index);
            topic(named(&t.name, t.topic_id, names), ps, f)
        }),
        Request::OffsetFetch(r) => offset_fetch(r, names, f),
        Request::Metadata(r) => r.topics.iter().flatten().any(|t| {
            let name = t.name.as_ref().map(|n| n.0.to_string());
            topic(name.or_else(|| names(t.topic_id)), std::iter::empty(), f)
        }),
        _ => false,
    }
}

fn offset_fetch(
    r: &kafka_wire::messages::OffsetFetchRequest,
    names: Names<'_>,
    f: &mut dyn FnMut(&str, Option<i32>) -> bool,
) -> bool {
    let old = r.topics.iter().flatten().any(|t| {
        topic(
            Some(t.name.0.to_string()),
            t.partition_indexes.iter().copied(),
            f,
        )
    });
    old || r
        .groups
        .iter()
        .flat_map(|g| g.topics.iter().flatten())
        .any(|t| {
            let ps = t.partition_indexes.iter().copied();
            topic(named(&t.name, t.topic_id, names), ps, f)
        })
}

/// One topic: `f` for each partition, or once with none if the entry names none.
fn topic(
    name: Option<String>,
    partitions: impl Iterator<Item = i32>,
    f: &mut dyn FnMut(&str, Option<i32>) -> bool,
) -> bool {
    let Some(name) = name else {
        return false;
    };
    let mut any = false;
    for p in partitions {
        any = true;
        if f(&name, Some(p)) {
            return true;
        }
    }
    !any && f(&name, None)
}

/// The group a request is about, if it is a group API.
pub(super) fn group_of(request: &Request) -> Option<String> {
    let id = match request {
        Request::JoinGroup(r) => &r.group_id.0,
        Request::SyncGroup(r) => &r.group_id.0,
        Request::Heartbeat(r) => &r.group_id.0,
        Request::LeaveGroup(r) => &r.group_id.0,
        Request::OffsetCommit(r) => &r.group_id.0,
        Request::OffsetFetch(r) if !r.group_id.0.is_empty() => &r.group_id.0,
        Request::OffsetFetch(r) => &r.groups.first()?.group_id.0,
        Request::ConsumerGroupHeartbeat(r) => &r.group_id.0,
        Request::FindCoordinator(r) if r.key_type == 0 && !r.key.is_empty() => &r.key,
        Request::FindCoordinator(r) if r.key_type == 0 => r.coordinator_keys.first()?,
        _ => return None,
    };
    Some(id.to_string())
}

/// The first topic (and partition) a request names, for the record of a rule's first hit.
pub(super) fn first_target(request: &Request, names: Names<'_>) -> Option<(Box<str>, Option<i32>)> {
    let mut found = None;
    any_target(request, names, &mut |t, p| {
        found = Some((Box::<str>::from(t), p));
        true
    });
    found
}
