use bytes::Bytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::{ApiKey, FindCoordinatorRequest, OffsetCommitRequest, OffsetFetchRequest};
use kafka_wire::profile::{KafkaMajor, Profile};
use kafka_wire::{StrBytes, response};

use super::*;
use crate::fault::testing::{fetch, heartbeat, metadata, produce};

/// Every error code the response carries, found by walking its encoded form's typed fields.
fn codes(r: &Response) -> Vec<i16> {
    match r {
        Response::Produce(r) => r
            .responses
            .iter()
            .flat_map(|t| t.partition_responses.iter().map(|p| p.error_code))
            .collect(),
        Response::Fetch(r) => r
            .responses
            .iter()
            .flat_map(|t| t.partitions.iter().map(|p| p.error_code))
            .collect(),
        Response::Metadata(r) => std::iter::once(r.error_code)
            .chain(r.topics.iter().map(|t| t.error_code))
            .collect(),
        Response::ListOffsets(r) => r
            .topics
            .iter()
            .flat_map(|t| t.partitions.iter().map(|p| p.error_code))
            .collect(),
        Response::FindCoordinator(r) => std::iter::once(r.error_code)
            .chain(r.coordinators.iter().map(|c| c.error_code))
            .collect(),
        Response::JoinGroup(r) => vec![r.error_code],
        Response::SyncGroup(r) => vec![r.error_code],
        Response::Heartbeat(r) => vec![r.error_code],
        Response::OffsetCommit(r) => r
            .topics
            .iter()
            .flat_map(|t| t.partitions.iter().map(|p| p.error_code))
            .collect(),
        Response::OffsetFetch(r) => std::iter::once(r.error_code)
            .chain(r.topics.iter().flat_map(|t| t.partitions.iter().map(|p| p.error_code)))
            .chain(r.groups.iter().flat_map(|g| {
                std::iter::once(g.error_code).chain(
                    g.topics
                        .iter()
                        .flat_map(|t| t.partitions.iter().map(|p| p.error_code)),
                )
            }))
            .collect(),
        Response::ConsumerGroupHeartbeat(r) => vec![r.error_code],
        Response::InitProducerId(r) => vec![r.error_code],
        other => panic!("not a failable response: {other:?}"),
    }
}

fn s(text: &str) -> StrBytes {
    StrBytes::from_string(text.to_owned())
}

/// One request of each failable API, with something in it for a code to go on.
fn requests() -> Vec<Request> {
    let mut list_offsets = kafka_wire::messages::ListOffsetsRequest::default();
    let mut t = kafka_wire::messages::list_offsets_request::ListOffsetsTopic::default();
    t.name = kafka_wire::messages::TopicName(s("orders"));
    t.partitions
        .push(kafka_wire::messages::list_offsets_request::ListOffsetsPartition::default());
    list_offsets.topics.push(t);
    let mut find = FindCoordinatorRequest::default();
    find.key = s("g");
    find.coordinator_keys = vec![s("g")];
    let mut commit = OffsetCommitRequest::default();
    let mut ct = kafka_wire::messages::offset_commit_request::OffsetCommitRequestTopic::default();
    ct.name = kafka_wire::messages::TopicName(s("orders"));
    ct.partitions
        .push(kafka_wire::messages::offset_commit_request::OffsetCommitRequestPartition::default());
    commit.topics.push(ct);
    let mut ofetch = OffsetFetchRequest::default();
    ofetch.topics = Some(vec![{
        let mut t = kafka_wire::messages::offset_fetch_request::OffsetFetchRequestTopic::default();
        t.name = kafka_wire::messages::TopicName(s("orders"));
        t.partition_indexes = vec![0, 1];
        t
    }]);
    ofetch.groups = vec![{
        let mut g = kafka_wire::messages::offset_fetch_request::OffsetFetchRequestGroup::default();
        g.topics = Some(vec![{
            let mut t =
                kafka_wire::messages::offset_fetch_request::OffsetFetchRequestTopics::default();
            t.partition_indexes = vec![0];
            t
        }]);
        g
    }];
    vec![
        produce("orders", &[0, 1], 1),
        fetch("orders", &[0, 1]),
        metadata(&["orders", "payments"]),
        Request::ListOffsets(list_offsets),
        Request::FindCoordinator(find),
        Request::JoinGroup(Default::default()),
        Request::SyncGroup(Default::default()),
        heartbeat("g"),
        Request::OffsetCommit(commit),
        Request::OffsetFetch(ofetch),
        Request::ConsumerGroupHeartbeat(Default::default()),
        Request::InitProducerId(Default::default()),
    ]
}

#[test]
fn every_failable_api_has_a_request_here_and_an_answer() {
    let reqs = requests();
    assert_eq!(reqs.len(), FAILABLE.len());
    for (req, api) in reqs.iter().zip(FAILABLE) {
        assert_eq!(req.api_key(), *api);
        assert!(can_fail(*api));
        assert!(fail_response(req, 0, 7).is_some(), "{api:?}");
    }
}

#[test]
fn the_code_is_in_every_place_a_client_reads_one() {
    for req in requests() {
        let api = req.api_key();
        let (low, high) = Profile::new(KafkaMajor::DEFAULT).range(api).unwrap();
        for version in low..=high {
            let r = fail_response(&req, version, 7).unwrap();
            let found = codes(&r);
            assert!(!found.is_empty(), "{api:?} v{version}");
            // The top-level code of a shape a version does not use stays 0: what the version's own field says is the code.
            assert!(found.contains(&7), "{api:?} v{version}: {found:?}");
            assert!(found.iter().all(|c| *c == 7 || *c == 0), "{api:?} v{version}: {found:?}");
        }
    }
}

#[test]
fn a_produce_answer_names_every_partition_asked_for_with_no_offset() {
    let req = produce("orders", &[0, 3], 1);
    let Some(Response::Produce(r)) = fail_response(&req, 9, 6) else {
        panic!("not a produce answer");
    };
    let t = r.responses.first().unwrap();
    assert_eq!(t.name.0.as_str(), "orders");
    let parts: Vec<(i32, i16, i64)> = t
        .partition_responses
        .iter()
        .map(|p| (p.index, p.error_code, p.base_offset))
        .collect();
    assert_eq!(parts, vec![(0, 6, -1), (3, 6, -1)]);
}

#[test]
fn an_old_offset_fetch_carries_the_code_at_the_top_and_a_new_one_in_each_group() {
    let reqs = requests();
    let req = reqs.iter().find(|r| r.api_key() == ApiKey::OffsetFetch).unwrap();
    let Some(Response::OffsetFetch(old)) = fail_response(req, 7, 7) else {
        panic!()
    };
    assert_eq!((old.error_code, old.groups.len(), old.topics.len()), (7, 0, 1));
    let Some(Response::OffsetFetch(new)) = fail_response(req, 9, 7) else {
        panic!()
    };
    assert_eq!((new.error_code, new.topics.len()), (0, 0));
    assert_eq!(new.groups.first().unwrap().error_code, 7);
}

#[test]
fn every_failable_answer_encodes_at_every_version_the_broker_serves() {
    for req in requests() {
        let api = req.api_key();
        for major in [KafkaMajor::DEFAULT] {
            let (low, high) = Profile::new(major).range(api).unwrap();
            for version in low..=high {
                let r = fail_response(&req, version, 3).unwrap();
                let bytes: Bytes = response::encode(1, version, &r)
                    .unwrap_or_else(|e| panic!("{api:?} v{version}: {e}"));
                assert!(bytes.len() > 4);
            }
        }
    }
}

#[test]
fn an_api_the_lab_cannot_answer_is_refused_not_guessed() {
    let create = Request::CreateTopics(Default::default());
    assert!(fail_response(&create, 7, 7).is_none());
    assert!(!can_fail(ApiKey::CreateTopics));
    assert!(!can_fail(ApiKey::SaslAuthenticate));
    assert!(!can_fail(ApiKey::ApiVersions));
    assert!(!can_fail(ApiKey::LeaveGroup));
}
