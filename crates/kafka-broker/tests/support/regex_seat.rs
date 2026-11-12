//! Members of the new protocol that keep on heartbeating, for the tests of regex subscriptions (SEC-210, SEC-211).
//! Included with `#[path = "support/regex_seat.rs"] mod seat;` after `mod support;`, `gx` and `calls`.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::collections::BTreeSet;
use std::time::Duration;

use crate::support::acls::admin;
use crate::support::{Rig, beat_848, join_848, rig_with};
use crate::{calls, gx};
use kafka_broker::Caller;
use kafka_wire::Uuid;

/// A broker that refuses by default, whose assignments are computed at every heartbeat.
pub fn world() -> Rig {
    rig_with(|c| {
        gx::closed(c);
        c.consumer_assignment_interval = Duration::ZERO;
    })
}

pub async fn id_of(rig: &Rig, name: &str) -> Uuid {
    rig.metadata_as(&admin(), 12, Some(&[name]), false)
        .await
        .topics[0]
        .topic_id
}

/// A member of the new protocol that keeps on heartbeating, owning what it was told to.
pub struct Seat<'a> {
    pub rig: &'a Rig,
    pub who: Caller,
    pub group: &'static str,
    pub member: &'static str,
    pub epoch: i32,
    pub owned: Vec<(Uuid, Vec<i32>)>,
    /// Every topic id it was ever assigned a partition of.
    pub ever: BTreeSet<Uuid>,
}

impl<'a> Seat<'a> {
    /// Joins with the regex (and no names).
    pub async fn join_regex(
        rig: &'a Rig,
        who: Caller,
        group: &'static str,
        member: &'static str,
        regex: &str,
    ) -> Seat<'a> {
        let mut req = join_848(group, member, &[]);
        req.subscribed_topic_names = None;
        req.subscribed_topic_regex = Some(gx::text(regex));
        let mut seat = Seat {
            rig,
            who,
            group,
            member,
            epoch: 0,
            owned: Vec::new(),
            ever: BTreeSet::new(),
        };
        let r = seat.send(req).await;
        assert_eq!(r, 0, "the join");
        seat.beats(4).await;
        seat
    }

    pub async fn send(&mut self, req: kafka_wire::messages::ConsumerGroupHeartbeatRequest) -> i16 {
        let r = calls::heartbeat_848(self.rig, &self.who, 1, req).await;
        if r.error_code == 0 {
            self.epoch = r.member_epoch;
            if let Some(a) = &r.assignment {
                self.owned = a
                    .topic_partitions
                    .iter()
                    .map(|t| (t.topic_id, t.partitions.clone()))
                    .collect();
                self.ever.extend(self.owned.iter().map(|(id, _)| *id));
            }
        }
        r.error_code
    }

    /// `n` plain heartbeats, each saying what the member owns.
    pub async fn beats(&mut self, n: usize) {
        for _ in 0..n {
            let owned = self.owned.clone();
            let req = beat_848(self.group, self.member, self.epoch, Some(&owned));
            assert_eq!(self.send(req).await, 0);
        }
    }

    pub fn owns(&self, id: Uuid) -> bool {
        self.owned.iter().any(|(t, _)| *t == id)
    }
}
