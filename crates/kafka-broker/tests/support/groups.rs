//! Requests for the group APIs.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::{Rig, at, head};
use bytes::Bytes;
use kafka_broker::Outcome;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::join_group_request::JoinGroupRequestProtocol;
use kafka_wire::messages::leave_group_request::MemberIdentity;
use kafka_wire::messages::sync_group_request::SyncGroupRequestAssignment;
use kafka_wire::messages::{
    FindCoordinatorRequest, FindCoordinatorResponse, GroupId, HeartbeatRequest, HeartbeatResponse,
    JoinGroupRequest, JoinGroupResponse, LeaveGroupRequest, LeaveGroupResponse, SyncGroupRequest,
    SyncGroupResponse,
};

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// A JoinGroup for a consumer: `protocols` are (name, metadata) most preferred first.
pub fn join_request(
    group: &str,
    member: &str,
    instance: Option<&str>,
    protocols: &[(&str, &str)],
) -> JoinGroupRequest {
    let mut req = JoinGroupRequest::default();
    req.group_id = GroupId(text(group));
    req.member_id = text(member);
    req.group_instance_id = instance.map(text);
    req.session_timeout_ms = 10_000;
    req.rebalance_timeout_ms = 30_000;
    req.protocol_type = text("consumer");
    req.protocols = protocols
        .iter()
        .map(|(n, m)| {
            let mut p = JoinGroupRequestProtocol::default();
            p.name = text(n);
            p.metadata = Bytes::from((*m).to_owned());
            p
        })
        .collect();
    req
}

impl Rig {
    pub async fn join_outcome(&self, version: i16, req: JoinGroupRequest) -> Outcome {
        let request = Request::JoinGroup(req);
        self.broker
            .handle(&at(), &head(request.api_key(), version), request)
            .await
    }

    pub async fn join(&self, version: i16, req: JoinGroupRequest) -> JoinGroupResponse {
        match self.join_outcome(version, req).await {
            Outcome::Respond(Response::JoinGroup(r)) => r,
            other => panic!("no join response: {other:?}"),
        }
    }

    pub async fn sync(
        &self,
        version: i16,
        group: &str,
        member: &str,
        generation: i32,
        assignments: &[(&str, &str)],
    ) -> SyncGroupResponse {
        let mut req = SyncGroupRequest::default();
        req.group_id = GroupId(text(group));
        req.member_id = text(member);
        req.generation_id = generation;
        req.assignments = assignments
            .iter()
            .map(|(m, a)| {
                let mut x = SyncGroupRequestAssignment::default();
                x.member_id = text(m);
                x.assignment = Bytes::from((*a).to_owned());
                x
            })
            .collect();
        match self.ask(version, Request::SyncGroup(req)).await {
            Response::SyncGroup(r) => r,
            other => panic!("not SyncGroup: {other:?}"),
        }
    }

    pub async fn heartbeat(&self, group: &str, member: &str, generation: i32) -> HeartbeatResponse {
        let mut req = HeartbeatRequest::default();
        req.group_id = GroupId(text(group));
        req.member_id = text(member);
        req.generation_id = generation;
        match self.ask(4, Request::Heartbeat(req)).await {
            Response::Heartbeat(r) => r,
            other => panic!("not Heartbeat: {other:?}"),
        }
    }

    pub async fn leave(&self, version: i16, group: &str, members: &[&str]) -> LeaveGroupResponse {
        let mut req = LeaveGroupRequest::default();
        req.group_id = GroupId(text(group));
        if version < 3 {
            req.member_id = text(members[0]);
        } else {
            req.members = members
                .iter()
                .map(|m| {
                    let mut i = MemberIdentity::default();
                    i.member_id = text(m);
                    i
                })
                .collect();
        }
        match self.ask(version, Request::LeaveGroup(req)).await {
            Response::LeaveGroup(r) => r,
            other => panic!("not LeaveGroup: {other:?}"),
        }
    }

    pub async fn find_coordinator(
        &self,
        version: i16,
        keys: &[&str],
        key_type: i8,
    ) -> FindCoordinatorResponse {
        let mut req = FindCoordinatorRequest::default();
        req.key_type = key_type;
        if version >= 4 {
            req.coordinator_keys = keys.iter().map(|k| text(k)).collect();
        } else {
            req.key = text(keys[0]);
        }
        match self.ask(version, Request::FindCoordinator(req)).await {
            Response::FindCoordinator(r) => r,
            other => panic!("not FindCoordinator: {other:?}"),
        }
    }
}

/// One partition of an OffsetCommit: (topic, partition, offset, metadata).
pub type Commit<'a> = (&'a str, i32, i64, Option<&'a str>);

impl Rig {
    pub async fn commit_offsets(
        &self,
        version: i16,
        group: &str,
        member: &str,
        generation: i32,
        commits: &[Commit<'_>],
    ) -> kafka_wire::messages::OffsetCommitResponse {
        use kafka_wire::messages::offset_commit_request::{
            OffsetCommitRequestPartition, OffsetCommitRequestTopic,
        };
        let mut req = kafka_wire::messages::OffsetCommitRequest::default();
        req.group_id = GroupId(text(group));
        req.member_id = text(member);
        req.generation_id_or_member_epoch = generation;
        for (name, partition, offset, metadata) in commits {
            let mut p = OffsetCommitRequestPartition::default();
            p.partition_index = *partition;
            p.committed_offset = *offset;
            p.committed_metadata = metadata.map(text);
            match req.topics.iter_mut().find(|t| t.name.as_str() == *name) {
                Some(t) => t.partitions.push(p),
                None => {
                    let mut t = OffsetCommitRequestTopic::default();
                    t.name = kafka_wire::messages::TopicName(text(name));
                    t.partitions.push(p);
                    req.topics.push(t);
                }
            }
        }
        match self.ask(version, Request::OffsetCommit(req)).await {
            Response::OffsetCommit(r) => r,
            other => panic!("not OffsetCommit: {other:?}"),
        }
    }

    /// What the group has committed: `asked` is the topics and partitions, or `None` for all there are. Returns
    /// (topic, partition, offset, metadata) in the order answered; `batched` uses the v8 form.
    pub async fn fetch_offsets(
        &self,
        batched: bool,
        group: &str,
        asked: Option<&[(&str, &[i32])]>,
    ) -> Vec<(String, i32, i64, String)> {
        use kafka_wire::messages::offset_fetch_request::{
            OffsetFetchRequestGroup, OffsetFetchRequestTopic, OffsetFetchRequestTopics,
        };
        let name = |n: &str| kafka_wire::messages::TopicName(text(n));
        let mut req = kafka_wire::messages::OffsetFetchRequest::default();
        if batched {
            let mut g = OffsetFetchRequestGroup::default();
            g.group_id = GroupId(text(group));
            g.topics = asked.map(|ts| {
                ts.iter()
                    .map(|(n, ps)| {
                        let mut t = OffsetFetchRequestTopics::default();
                        t.name = name(n);
                        t.partition_indexes = ps.to_vec();
                        t
                    })
                    .collect()
            });
            req.groups.push(g);
        } else {
            req.group_id = GroupId(text(group));
            req.topics = asked.map(|ts| {
                ts.iter()
                    .map(|(n, ps)| {
                        let mut t = OffsetFetchRequestTopic::default();
                        t.name = name(n);
                        t.partition_indexes = ps.to_vec();
                        t
                    })
                    .collect()
            });
        }
        let version = if batched { 8 } else { 7 };
        let Response::OffsetFetch(r) = self.ask(version, Request::OffsetFetch(req)).await else {
            panic!("not OffsetFetch");
        };
        let mut out = Vec::new();
        for t in &r.topics {
            for p in &t.partitions {
                assert_eq!(p.error_code, 0);
                out.push((
                    t.name.to_string(),
                    p.partition_index,
                    p.committed_offset,
                    p.metadata
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                ));
            }
        }
        for g in &r.groups {
            for t in &g.topics {
                for p in &t.partitions {
                    assert_eq!(p.error_code, 0);
                    out.push((
                        t.name.to_string(),
                        p.partition_index,
                        p.committed_offset,
                        p.metadata
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_default(),
                    ));
                }
            }
        }
        out
    }
}
