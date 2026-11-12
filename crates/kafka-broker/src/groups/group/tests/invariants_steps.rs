//! The random steps of the invariant generator: a request of a consumer-protocol client, a request of a classic one, an ACL
//! change, a topic change, time passing, an admin's LeaveGroup.

use kafka_wire::ids::InstanceId;

use super::invariants::{P, World, rights_under};
use super::sim::mid;
use super::*;
use crate::authorizer::TopicRights;
use crate::groups::consumer::Heartbeat;
use crate::groups::machine::LeaveTarget;

pub(super) const T: [&str; 5] = ["a0", "a1", "b0", "b1", "c0"];

impl World {
    pub(super) fn topic_names(&mut self, at_least: usize, spread: usize) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for _ in 0..at_least + self.rng.below(spread) {
            let t = T[self.rng.below(5)].to_owned();
            if !v.contains(&t) {
                v.push(t);
            }
        }
        v
    }

    /// The principal of a client: its usual one mostly, another one some of the time.
    fn principal_of(&mut self, name: &str) -> usize {
        let usual = usize::from(name.as_bytes()[1]) % 4;
        if self.rng.pct(85) {
            usual
        } else {
            self.rng.below(4)
        }
    }

    /// The epoch a consumer-protocol client sends: 0 joins, -1 leaves, -2 leaves to come back, otherwise its own.
    fn epoch_to_send(&mut self, known: i32) -> i32 {
        let r = self.rng.below(100);
        match r {
            _ if known == 0 || r < 6 => 0,
            6..=89 => known,
            90..=93 => -1,
            _ => -2,
        }
    }

    fn heartbeat_for(
        &mut self,
        member: &str,
        instance: Option<String>,
        epoch: i32,
        rights: &Option<TopicRights>,
    ) -> Heartbeat {
        let subs = self.topic_names(0, 4);
        let regex = self
            .rng
            .pct(35)
            .then(|| ["a.*", "b.", ".*0", "c.*"][self.rng.below(4)].to_owned());
        let change = epoch == 0 || self.rng.pct(10);
        let rebalance_ms = if epoch == 0 {
            [5_000, 30_000][self.rng.below(2)]
        } else {
            -1
        };
        let owned = self.owned.get(member).cloned().unwrap_or_default();
        Heartbeat {
            group: "g".into(),
            member_id: Some(mid(member)),
            version: 1,
            epoch,
            instance: instance.as_deref().map(InstanceId::new),
            rack: None,
            rebalance_ms,
            subscribed: change.then_some(subs),
            regex: change.then(|| regex.unwrap_or_default()),
            assignor: None,
            owned: if self.rng.pct(8) { None } else { Some(owned) },
            client_id: "c".into(),
            client_host: "/h".into(),
            rights: rights.clone(),
        }
    }

    pub(super) fn cons_request(&mut self) {
        let n = format!("c{}", self.rng.below(5));
        let who = self.principal_of(&n);
        let rights = self.rights[who].clone();
        let known = self.epoch.get(&n).copied().unwrap_or(0);
        let epoch = self.epoch_to_send(known);
        // The first two have a static instance; a join may be a takeover of it under another member id.
        let instance = (n == "c0" || n == "c1").then(|| format!("inst-{n}"));
        let member = if instance.is_some() && epoch == 0 && self.rng.pct(40) {
            format!("{n}x{}", self.rng.below(3))
        } else {
            n.clone()
        };
        if epoch == 0 {
            self.epoch.insert(member.clone(), 0);
        }
        let hb = self.heartbeat_for(&member, instance, epoch, &rights);
        self.trace.push(format!(
            "848 {member} as {} epoch {epoch} inst {:?} subs {:?} regex {:?}",
            P[who], hb.instance, hb.subscribed, hb.regex
        ));
        let t = self.send(|t| Event::ConsumerHeartbeat(t, hb));
        if let Some(a) = self.take(t) {
            if matches!(&a, Answer::ConsumerHeartbeat(h) if h.error.is_none()) && epoch >= 0 {
                self.stale_ok.remove(&member);
                self.last.insert(member.clone(), rights.clone());
            }
            self.learn_848(&member, &rights, a);
        }
        if epoch < 0 {
            self.epoch.remove(&member);
        }
        self.check_all("848");
    }

    pub(super) fn classic_request(&mut self) {
        let n = format!("k{}", self.rng.below(4));
        let who = self.principal_of(&n);
        let rights = self.rights[who].clone();
        match self.rng.below(100) {
            0..=29 => self.classic_join(&n, who, &rights),
            30..=59 => self.classic_sync(&n),
            60..=89 => self.classic_beat(&n, who, &rights),
            _ => self.classic_leave(&n),
        }
        self.check_all("classic");
    }

    fn acl_change(&mut self) {
        let p = self.rng.below(4);
        let t = T[self.rng.below(5)];
        match self.rng.below(6) {
            0 => self.acls.0.push((p, t, false, true)),
            1 | 2 => self.acls.0.push((p, t, false, false)),
            3 => {
                let prefix = ["a", "b", "c"][self.rng.below(3)];
                let allow = self.rng.pct(50);
                self.acls.0.push((p, prefix, true, allow));
            }
            4 => self.acls.0.retain(|(_, _, _, allow)| !allow),
            _ => self.acls.0.retain(|(_, _, _, allow)| *allow),
        }
        self.rights = rights_under(&self.acls, self.super_user, self.default_allow);
        self.trace.push(format!(
            "ACL change {:?} (super {:?} default_allow {})",
            self.acls.0.last(),
            self.super_user,
            self.default_allow
        ));
    }

    fn topics_change(&mut self) {
        let t = T[self.rng.below(5)];
        match self.rng.below(3) {
            0 if !self.topics.contains_key(t) => self.make(t),
            1 => {
                self.topics.remove(t);
            }
            _ => {
                if let Some((_, p)) = self.topics.get_mut(t) {
                    *p += 1;
                }
            }
        }
        self.trace.push("topics changed".into());
        self.send(|_| Event::TopicsChanged);
        self.check_all("topics");
    }

    /// An admin removes a member of either protocol.
    fn admin_leave(&mut self) {
        let n = if self.rng.pct(50) {
            format!("c{}", self.rng.below(5))
        } else {
            format!("k{}", self.rng.below(4))
        };
        let target = LeaveTarget {
            member_id: Some(mid(&n)),
            instance: None,
        };
        self.trace.push(format!("admin leave {n}"));
        let t = self.send(|t| Event::Leave(t, vec![target]));
        self.take(t);
        self.check_all("admin leave");
    }

    pub(super) fn step(&mut self) {
        match self.rng.below(100) {
            0..=34 => self.cons_request(),
            35..=59 => self.classic_request(),
            60..=74 => self.acl_change(),
            75..=84 => {
                let ms = [200_u64, 600, 1_200, 3_500, 9_000, 50_000][self.rng.below(6)];
                self.advance(ms);
            }
            85..=92 => self.topics_change(),
            _ => self.admin_leave(),
        }
    }
}
