//! Which groups read a topic: ListGroups, then each group's committed offsets.

use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::messages::offset_fetch_request::OffsetFetchRequestGroup;
use kafka_wire::messages::{GroupId, ListGroupsRequest, OffsetFetchRequest};

/// Every group the broker knows.
#[must_use]
pub fn list_groups() -> Request {
    Request::ListGroups(ListGroupsRequest::default())
}

/// The committed offsets of these groups, for every topic and partition they have committed (no topic list asked for).
/// At most 50 groups.
#[must_use]
pub fn committed_of(groups: &[String]) -> Request {
    let mut r = OffsetFetchRequest::default();
    r.require_stable = false;
    r.groups = groups
        .iter()
        .take(50)
        .map(|g| {
            let mut x = OffsetFetchRequestGroup::default();
            x.group_id = GroupId(StrBytes::from_string(g.clone()));
            x.topics = None;
            x
        })
        .collect();
    Request::OffsetFetch(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_group_is_asked_for_all_it_has_committed_and_the_ask_is_bounded() {
        let names: Vec<String> = (0..80).map(|i| format!("g{i}")).collect();
        let Request::OffsetFetch(r) = committed_of(&names) else {
            panic!("not OffsetFetch");
        };
        assert_eq!(r.groups.len(), 50);
        assert!(r.groups.iter().all(|g| g.topics.is_none()));
        assert!(!r.require_stable);
    }
}
