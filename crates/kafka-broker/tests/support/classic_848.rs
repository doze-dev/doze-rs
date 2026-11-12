//! Classic members (JoinGroup, SyncGroup, Heartbeat) put to a broker as a given caller, for the tests of what a consumer
//! group tells them (SEC-231, SEC-234). Included with `#[path = "support/classic_848.rs"] mod classic;` after `mod support;`
//! and `gx`.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use bytes::Bytes;

use crate::gx::{group_id, text};
use crate::support::{Rig, join_request, subscription};
use kafka_broker::Caller;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::sync_group_request::SyncGroupRequestAssignment;
use kafka_wire::messages::{HeartbeatRequest, SyncGroupRequest};

/// The bytes a consumer is given as its assignment: topics with partitions, version 0, no user data.
pub fn assignment_bytes(topics: &[(&str, &[i32])]) -> Bytes {
    let mut out = vec![0_u8, 0];
    out.extend_from_slice(&(topics.len() as i32).to_be_bytes());
    for (name, partitions) in topics {
        out.extend_from_slice(&(name.len() as i16).to_be_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&(partitions.len() as i32).to_be_bytes());
        for p in *partitions {
            out.extend_from_slice(&p.to_be_bytes());
        }
    }
    out.extend_from_slice(&(-1_i32).to_be_bytes());
    Bytes::from(out)
}

/// What an assignment holds: the topics by name with their partitions.
pub fn read_assignment(bytes: &[u8]) -> Vec<(String, Vec<i32>)> {
    if bytes.len() < 6 {
        return Vec::new();
    }
    let int = |at: usize| i32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    let topics = int(2);
    let mut at = 6;
    let mut out = Vec::new();
    for _ in 0..topics {
        let len = i16::from_be_bytes(bytes[at..at + 2].try_into().unwrap()) as usize;
        let name = String::from_utf8(bytes[at + 2..at + 2 + len].to_vec()).unwrap();
        at += 2 + len;
        let count = int(at) as usize;
        at += 4;
        let ps = (0..count).map(|i| int(at + 4 * i)).collect();
        at += 4 * count;
        out.push((name, ps));
    }
    out
}

/// A classic member that has joined.
pub struct Classic {
    pub who: Caller,
    pub group: &'static str,
    pub id: String,
    pub generation: i32,
    pub error: i16,
}

/// A JoinGroup (version 4: an id is handed out first) of `who` subscribing to `names`; `id` is its member id if it has one.
pub async fn join(
    rig: &Rig,
    who: &Caller,
    group: &'static str,
    id: &str,
    instance: Option<&str>,
    names: &[&str],
) -> Classic {
    let meta = subscription(names);
    let mut id = id.to_owned();
    for _ in 0..2 {
        let req = join_request(group, &id, instance, &[("range", &meta)]);
        let Response::JoinGroup(r) = rig.ask_as(who, 4, Request::JoinGroup(req)).await else {
            panic!("not JoinGroup")
        };
        if r.error_code == 79 {
            id = r.member_id.to_string();
            continue;
        }
        return Classic {
            who: who.clone(),
            group,
            id: r.member_id.to_string(),
            generation: r.generation_id,
            error: r.error_code,
        };
    }
    panic!("the id was handed out twice")
}

impl Classic {
    /// Its SyncGroup, carrying `assignments` (a leader's); the assignment it is given, by topic name.
    pub async fn sync(
        &self,
        rig: &Rig,
        assignments: &[(&str, Bytes)],
    ) -> (i16, Vec<(String, Vec<i32>)>) {
        let mut req = SyncGroupRequest::default();
        req.group_id = group_id(self.group);
        req.member_id = text(&self.id);
        req.generation_id = self.generation;
        req.assignments = assignments
            .iter()
            .map(|(m, a)| {
                let mut x = SyncGroupRequestAssignment::default();
                x.member_id = text(m);
                x.assignment = a.clone();
                x
            })
            .collect();
        let Response::SyncGroup(r) = rig.ask_as(&self.who, 4, Request::SyncGroup(req)).await else {
            panic!("not SyncGroup")
        };
        (r.error_code, read_assignment(&r.assignment))
    }

    /// Its Heartbeat's error code.
    pub async fn beat(&self, rig: &Rig) -> i16 {
        let mut req = HeartbeatRequest::default();
        req.group_id = group_id(self.group);
        req.member_id = text(&self.id);
        req.generation_id = self.generation;
        let Response::Heartbeat(r) = rig.ask_as(&self.who, 4, Request::Heartbeat(req)).await else {
            panic!("not Heartbeat")
        };
        r.error_code
    }
}
