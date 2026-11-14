//! The requests of the classic members in the invariant generator: JoinGroup, SyncGroup, Heartbeat and LeaveGroup.

use std::collections::BTreeMap;

use kafka_wire::ids::InstanceId;

use super::invariants::{P, World};
use super::sim::{may, mid, subscription_of};
use super::*;
use crate::authorizer::TopicRights;
use crate::groups::consumer::protocol::decode_assignment;
use crate::groups::machine::LeaveTarget;

impl World {
    pub(super) fn classic_join(&mut self, n: &str, who: usize, rights: &Option<TopicRights>) {
        let subs = self.topic_names(1, 4);
        let owned = if self.rng.pct(50) {
            self.held.get(n).cloned().unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        let instance = (n == "k0" && self.rng.pct(50)).then(|| InstanceId::new("ci"));
        let by_instance = instance.is_some() && self.rng.pct(50);
        let req = JoinRequest {
            reason: None,
            member_id: (!by_instance).then(|| mid(n)),
            instance,
            protocol_type: "consumer".into(),
            protocols: vec![Protocol {
                name: "range".into(),
                metadata: subscription_of(1, &subs, &owned),
            }],
            session_ms: 30_000,
            rebalance_ms: 60_000,
            client_id: "c".into(),
            client_host: "/h".into(),
            issued: true,
            id_first: false,
            rights: rights.clone(),
        };
        self.trace.push(format!(
            "classic JOIN {n} as {} member_id {:?} subs {subs:?} owned {owned:?}",
            P[who], req.member_id
        ));
        self.send_join(n, rights, req);
    }

    /// A SyncGroup carries no rights: what it is given is checked against the member's last presented.
    pub(super) fn classic_sync(&mut self, n: &str) {
        let generation = self.gn.get(n).copied().unwrap_or(0);
        let req = SyncRequest {
            member_id: mid(n),
            instance: None,
            generation,
            protocol_type: None,
            protocol: None,
            assignments: Vec::new(),
        };
        self.trace
            .push(format!("classic SYNC {n} gen {generation}"));
        let t = self.send(|t| Event::Sync(t, req));
        let Some(Answer::Sync(s)) = self.take(t) else {
            return;
        };
        if s.error.is_some() {
            return;
        }
        let last = self.last.get(n).cloned().unwrap_or(None);
        let got = decode_assignment(&s.assignment).unwrap_or_default();
        for (name, ps) in got.iter().filter(|(name, _)| !may(&last, name)) {
            self.bad(format!(
                "#{}: classic {n} was SYNCED {name}/{ps:?} hidden from its last rights",
                self.events
            ));
        }
        self.held.insert(n.to_owned(), got);
    }

    pub(super) fn classic_beat(&mut self, n: &str, who: usize, rights: &Option<TopicRights>) {
        let generation = self.gn.get(n).copied().unwrap_or(0);
        let req = HeartbeatRequest {
            member_id: mid(n),
            instance: None,
            generation,
            rights: rights.clone(),
        };
        let consumer_kind = self.pump.group.consumer_life().is_some();
        self.trace.push(format!(
            "classic BEAT {n} as {} gen {generation} (consumer kind {consumer_kind})",
            P[who]
        ));
        let t = self.send(|t| Event::Heartbeat(t, req));
        if let Some(Answer::Heartbeat(e)) = self.take(t)
            && consumer_kind
            && (e.is_none() || e == Some(E::RebalanceInProgress))
        {
            self.stale_ok.remove(n);
            self.last.insert(n.to_owned(), rights.clone());
        }
    }

    pub(super) fn classic_leave(&mut self, n: &str) {
        let target = LeaveTarget {
            reason: None,
            member_id: Some(mid(n)),
            instance: None,
        };
        self.trace.push(format!("classic LEAVE {n}"));
        let t = self.send(|t| Event::Leave(t, vec![target]));
        self.take(t);
        self.held.remove(n);
    }
}
