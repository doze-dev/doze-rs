//! One member of a streams group: a stream thread of a Kafka Streams instance.

use std::collections::BTreeMap;

use kafka_wire::ids::{InstanceId, MemberId};

use super::reconcile::Held;
use super::{Endpoint, StreamsHeartbeat, Task};
use crate::groups::machine::Millis;

#[derive(Debug, Clone)]
pub(super) struct Member {
    pub(super) id: MemberId,
    pub(super) instance: Option<InstanceId>,
    pub(super) rack: Option<String>,
    pub(super) client_id: String,
    pub(super) client_host: String,
    pub(super) rebalance_ms: i32,
    /// The topology epoch it joined with.
    pub(super) topology_epoch: i32,
    /// The Kafka Streams instance it is a thread of.
    pub(super) process: String,
    pub(super) endpoint: Option<Endpoint>,
    pub(super) client_tags: BTreeMap<String, String>,
    pub(super) held: Held,
    /// The changelog offsets it last reported, by task (memory only, as in Kafka).
    pub(super) offsets: BTreeMap<Task, i64>,
    pub(super) end_offsets: BTreeMap<Task, i64>,
    pub(super) session_gen: u64,
    /// The session it was last given; only the console reads it.
    #[cfg_attr(not(feature = "inspect"), allow(dead_code))]
    pub(super) session_ms: u64,
    /// The arming of its rebalance timer that counts, and the epoch it was armed at (0: none).
    pub(super) revoke_gen: u64,
    pub(super) revoke_epoch: i32,
    /// When it was last heard from; only the console reads it.
    #[cfg_attr(not(feature = "inspect"), allow(dead_code))]
    pub(super) heard: Millis,
}

impl Member {
    /// A member as Kafka makes one before a request fills it in (`getOrCreateDefaultMember`).
    pub(super) fn new(id: MemberId) -> Self {
        Self {
            id,
            instance: None,
            rack: None,
            client_id: String::new(),
            client_host: String::new(),
            rebalance_ms: -1,
            topology_epoch: -1,
            process: String::new(),
            endpoint: None,
            client_tags: BTreeMap::new(),
            held: Held::default(),
            offsets: BTreeMap::new(),
            end_offsets: BTreeMap::new(),
            session_gen: 0,
            session_ms: 0,
            revoke_gen: 0,
            revoke_epoch: 0,
            heard: Millis(0),
        }
    }

    /// Takes what the heartbeat says of the member, and says what changed.
    pub(super) fn update(&mut self, hb: &StreamsHeartbeat) -> Changed {
        // What every member's epoch depends on, and what only a dynamic member's does.
        let (mut counts, mut dynamic, mut endpoint) = (false, false, false);
        if let Some(i) = hb.instance.as_ref().filter(|i| self.instance.as_ref() != Some(*i)) {
            self.instance = Some(i.clone());
            dynamic = true;
        }
        if let Some(r) = hb.rack.as_ref().filter(|r| self.rack.as_ref() != Some(*r)) {
            self.rack = Some(r.clone());
            counts = true;
        }
        if hb.rebalance_ms != -1 && hb.rebalance_ms != self.rebalance_ms {
            self.rebalance_ms = hb.rebalance_ms;
            dynamic = true;
        }
        dynamic |= replace(&mut self.client_id, &hb.client_id);
        dynamic |= replace(&mut self.client_host, &hb.client_host);
        if let Some(t) = hb.topology.as_ref().filter(|t| t.epoch != self.topology_epoch) {
            self.topology_epoch = t.epoch;
            counts = true;
        }
        if let Some(p) = &hb.process {
            counts |= replace(&mut self.process, p);
        }
        // A join sets the endpoint, even to none; later a heartbeat changes it only by sending one.
        if (hb.epoch == 0 || hb.endpoint.is_some()) && self.endpoint != hb.endpoint {
            self.endpoint.clone_from(&hb.endpoint);
            endpoint = true;
        }
        if let Some(tags) = hb.client_tags.as_ref().filter(|t| **t != self.client_tags) {
            self.client_tags.clone_from(tags);
            counts = true;
        }
        Changed {
            group: counts || ((dynamic || endpoint) && self.instance.is_none()),
            endpoint,
        }
    }
}

/// What a heartbeat changed of a member.
#[derive(Debug, Clone, Copy)]
pub(super) struct Changed {
    /// Something the group's epoch depends on (Kafka's `hasStreamsMemberMetadataChanged`: for a static member only its
    /// topology epoch, rack, tags or process).
    pub(super) group: bool,
    /// Its endpoint.
    pub(super) endpoint: bool,
}

/// Sets `into` to `value`; `true` if that changed it.
fn replace(into: &mut String, value: &str) -> bool {
    if into == value {
        return false;
    }
    value.clone_into(into);
    true
}
