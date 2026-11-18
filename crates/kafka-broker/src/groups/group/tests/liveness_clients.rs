//! What the simulated clients do each 500 ms: a consumer-protocol client heartbeats, a classic one joins, syncs and
//! heartbeats, and both do what the answer says.

use std::collections::BTreeMap;

use super::liveness_sim::{Kind, Phase, Sim};
use super::sim::subscription_of;
use super::*;
use crate::authorizer::TopicRights;
use crate::groups::consumer::Heartbeat;
use crate::groups::consumer::protocol::{ByName, decode_assignment};

impl Sim {
    /// One 500 ms tick: every client once, in a rotating order.
    pub(super) fn tick(&mut self) {
        let n = self.clients.len();
        let start = self.rng.below(n.max(1));
        for k in 0..n {
            self.tick_client((start + k) % n);
        }
        self.advance(500);
    }

    fn count(&mut self, what: String) {
        *self.errors.entry(what).or_default() += 1;
    }

    fn tick_client(&mut self, i: usize) {
        let rights = self.rights[self.clients[i].who].clone();
        match self.clients[i].kind.clone() {
            Kind::Cons { regex } => self.tick_consumer(i, regex, rights),
            Kind::Classic { coop } => match self.clients[i].phase.clone() {
                Phase::NeedJoin => self.classic_join(i, coop, rights),
                Phase::NeedSync(g) => self.classic_sync(i, g),
                Phase::Active(g) => self.classic_beat(i, g, rights),
            },
        }
    }

    fn tick_consumer(&mut self, i: usize, regex: Option<String>, rights: Option<TopicRights>) {
        let c = &self.clients[i];
        let joining = c.epoch == 0;
        let hb = Heartbeat {
            group: "g".into(),
            member_id: Some(self.member(i)),
            version: 1,
            epoch: c.epoch,
            instance: None,
            rack: None,
            rebalance_ms: if joining { 30_000 } else { -1 },
            subscribed: joining.then(|| c.subs.clone()),
            regex: joining.then(|| regex.unwrap_or_default()),
            assignor: None,
            owned: Some(c.owned.clone()),
            client_id: "c".into(),
            client_host: "/h".into(),
            rights,
        };
        let t = self.send(|t| Event::ConsumerHeartbeat(t, hb));
        let Some(Answer::ConsumerHeartbeat(h)) = self.answer(t) else {
            return;
        };
        if let Some((e, _)) = &h.error {
            self.count(format!("cons:{e:?}"));
            let c = &mut self.clients[i];
            c.epoch = 0;
            c.owned.clear();
            return;
        }
        let c = &mut self.clients[i];
        c.epoch = h.epoch;
        if let Some(assignment) = &h.assignment {
            let mut by: BTreeMap<Uuid, Vec<i32>> = BTreeMap::new();
            for (t, p) in assignment {
                by.entry(*t).or_default().push(*p);
            }
            c.owned = by.into_iter().collect();
        }
    }

    fn classic_join(&mut self, i: usize, coop: bool, rights: Option<TopicRights>) {
        let c = &self.clients[i];
        let owned = if coop { c.held.clone() } else { ByName::new() };
        let req = JoinRequest {
            reason: None,
            member_id: Some(self.member(i)),
            instance: None,
            protocol_type: "consumer".into(),
            protocols: vec![Protocol {
                name: "range".into(),
                metadata: subscription_of(1, &c.subs, &owned),
            }],
            session_ms: 30_000,
            rebalance_ms: 60_000,
            client_id: "c".into(),
            client_host: "/h".into(),
            issued: true,
            id_first: false,
            rights,
        };
        let t = self.send(|t| Event::Join(t, req));
        match self.answer(t) {
            Some(Answer::Join(j)) => match j.error {
                None => self.clients[i].phase = Phase::NeedSync(j.generation),
                Some(e) => self.count(format!("join:{e:?}")),
            },
            _ => self.count("join:parked".into()),
        }
    }

    fn classic_sync(&mut self, i: usize, generation: i32) {
        let req = SyncRequest {
            member_id: self.member(i),
            instance: None,
            generation,
            protocol_type: None,
            protocol: None,
            assignments: Vec::new(),
        };
        let t = self.send(|t| Event::Sync(t, req));
        match self.answer(t) {
            Some(Answer::Sync(s)) if s.error.is_none() => {
                let c = &mut self.clients[i];
                c.held = decode_assignment(&s.assignment).unwrap_or_default();
                c.phase = Phase::Active(generation);
            }
            Some(Answer::Sync(s)) => {
                self.count(format!("sync:{:?}", s.error));
                self.clients[i].phase = Phase::NeedJoin;
            }
            _ => {}
        }
    }

    fn classic_beat(&mut self, i: usize, generation: i32, rights: Option<TopicRights>) {
        let req = HeartbeatRequest {
            member_id: self.member(i),
            instance: None,
            generation,
            rights,
        };
        let t = self.send(|t| Event::Heartbeat(t, req));
        let Some(Answer::Heartbeat(Some(e))) = self.answer(t) else {
            return;
        };
        self.count(format!("beat:{e:?}"));
        if e == E::RebalanceInProgress {
            self.rebalance_signals += 1;
        } else {
            self.clients[i].held.clear();
        }
        self.clients[i].phase = Phase::NeedJoin;
    }
}
