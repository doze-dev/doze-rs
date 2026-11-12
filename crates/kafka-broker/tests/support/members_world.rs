//! The world of the random test of members (`authz_members_random_848.rs`): three users, a pool of topics, the members of
//! the new protocol and the classic members of one group, and the check of what each was told against what its principal
//! may DESCRIBE. Included with `#[path = "support/members_world.rs"] mod world;` after the other support modules.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::collections::{BTreeMap, BTreeSet};

use crate::gx::{self, ALLOW, DENY, DESCRIBE, LITERAL, PREFIXED, TOPIC, creation};
use crate::seat::Seat;
use crate::support::acls::{admin, filter, user};
use crate::support::{Rig, join_848};
use crate::{calls, classic};

pub const POOL: [&str; 6] = ["t0", "t1", "t2", "t3", "x0", "x1"];
pub const USERS: [&str; 3] = ["alice", "bob", "carol"];
pub const REGEXES: [&str; 3] = ["t.*", ".*", "t[0-3]|x.*"];
pub const SEEDS: u64 = 220;
pub const STEPS: usize = 40;

pub struct Rng(pub u64);
impl Rng {
    pub fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// For each user, the pool names it may DESCRIBE now (a topic that is not there is described to whoever may).
pub async fn visible(rig: &Rig) -> Vec<BTreeSet<String>> {
    let mut out = Vec::new();
    for u in USERS {
        let r = rig.metadata_as(&user(u), 12, Some(&POOL), false).await;
        let names = r.topics.iter().filter(|t| t.error_code != 29);
        out.push(
            names
                .filter_map(|t| t.name.as_ref().map(|n| n.0.to_string()))
                .collect(),
        );
    }
    out
}

pub async fn names_by_id(rig: &Rig) -> BTreeMap<kafka_wire::Uuid, String> {
    let all = rig.metadata_as(&admin(), 12, None, false).await;
    let named = all
        .topics
        .iter()
        .filter_map(|t| Some((t.topic_id, t.name.as_ref()?.0.to_string())));
    named.collect()
}

/// A subscription of names the user may DESCRIBE now (those picked from the pool).
pub fn pick(r: &mut Rng, may: &BTreeSet<String>) -> Vec<String> {
    POOL.iter()
        .filter(|t| may.contains(**t) && r.below(2) == 0)
        .map(|t| (*t).to_owned())
        .collect()
}

pub fn as_refs(names: &[String]) -> Vec<&str> {
    names.iter().map(String::as_str).collect()
}

pub struct World<'a> {
    pub rig: &'a Rig,
    /// (user index, seat) of every member of the new protocol.
    pub seats: Vec<(usize, Seat<'a>)>,
    pub static_slot: Option<usize>,
    pub classic: Vec<Option<classic::Classic>>,
    pub classic_names: Vec<Vec<String>>,
    /// What the classic members were given by SyncGroup since the last check: (user index, topic names).
    pub synced: Vec<(usize, Vec<String>)>,
    pub made: Vec<&'static str>,
    pub taken_over: usize,
}

impl<'a> World<'a> {
    pub async fn acl_change(&mut self, r: &mut Rng) {
        let u = USERS[r.below(3)];
        let (name, pat) = if r.below(4) == 0 {
            ("t", PREFIXED)
        } else {
            (POOL[r.below(POOL.len())], LITERAL)
        };
        let make = |perm| creation((TOPIC, name, pat), &format!("User:{u}"), DESCRIBE, perm);
        match r.below(6) {
            0 => drop(
                self.rig
                    .delete_acls_as(&admin(), 3, vec![filter(TOPIC, 1, DESCRIBE, DENY)])
                    .await,
            ),
            1 => drop(
                self.rig
                    .delete_acls_as(&admin(), 3, vec![filter(TOPIC, 1, DESCRIBE, ALLOW)])
                    .await,
            ),
            2 | 3 => drop(self.rig.create_acls_as(&admin(), 3, vec![make(DENY)]).await),
            _ => drop(
                self.rig
                    .create_acls_as(&admin(), 3, vec![make(ALLOW)])
                    .await,
            ),
        }
    }

    /// The instance `i1` is taken by a random principal: with names it may describe, or with an expression that matches nothing.
    pub async fn takeover(&mut self, r: &mut Rng) {
        let u = r.below(3);
        let may = visible(self.rig).await.swap_remove(u);
        let names = pick(r, &may);
        self.taken_over += 1;
        let member: &'static str = Box::leak(format!("s{}", self.taken_over).into_boxed_str());
        let mut req = join_848("g", member, &as_refs(&names));
        if r.below(2) == 0 {
            req.subscribed_topic_names = None;
            req.subscribed_topic_regex = Some(gx::text("never-matches-x"));
        }
        if let Some(i) = self.static_slot {
            self.seats[i].1.leave_for_now().await;
        }
        let seat = Seat::start(self.rig, user(USERS[u]), "g", member, Some("i1"), req).await;
        match self.static_slot {
            Some(i) => self.seats[i] = (u, seat),
            None => {
                self.static_slot = Some(self.seats.len());
                self.seats.push((u, seat));
            }
        }
    }

    /// A classic member of a user joins (again) naming topics from the pool, hidden or not, and syncs.
    pub async fn classic_rejoin(&mut self, r: &mut Rng) {
        let u = r.below(3);
        let names: Vec<String> = POOL
            .iter()
            .filter(|_| r.below(2) == 0)
            .map(|t| (*t).to_owned())
            .collect();
        self.classic_join(u, names).await;
    }

    pub async fn classic_join(&mut self, u: usize, names: Vec<String>) {
        let id = self.classic[u]
            .as_ref()
            .map_or(String::new(), |c| c.id.clone());
        let joined =
            classic::join(self.rig, &user(USERS[u]), "g", &id, None, &as_refs(&names)).await;
        if joined.error == 0 {
            let (code, got) = joined.sync(self.rig, &[]).await;
            if code == 0 {
                self.synced
                    .push((u, got.into_iter().map(|(t, _)| t).collect()));
            }
            self.classic[u] = Some(joined);
            self.classic_names[u] = names;
        }
    }

    /// Every member heartbeats; a classic member told to rejoin does.
    pub async fn settle(&mut self) {
        for (_, seat) in &mut self.seats {
            seat.beats(6).await;
        }
        for u in 0..3 {
            let told = match &self.classic[u] {
                Some(c) => c.beat(self.rig).await,
                None => 0,
            };
            if told == 27 {
                self.classic_join(u, self.classic_names[u].clone()).await;
            }
        }
    }

    /// What every member was told, owns and is shown, against what its own principal may DESCRIBE. Returns the count.
    pub async fn check(&mut self, tag: &str, bad: &mut Vec<String>) -> usize {
        let (may, ids) = (visible(self.rig).await, names_by_id(self.rig).await);
        let mut seen = 0;
        for (u, seat) in &mut self.seats {
            let told: BTreeSet<_> = std::mem::take(&mut seat.fresh)
                .into_iter()
                .chain(seat.owned.iter().map(|(id, _)| *id))
                .collect();
            for id in told {
                seen += 1;
                let name = ids.get(&id).cloned().unwrap_or_default();
                if !may[*u].contains(&name) {
                    bad.push(format!(
                        "{tag}: {} ({}) was told {name}",
                        USERS[*u], seat.member
                    ));
                }
            }
        }
        for (u, names) in std::mem::take(&mut self.synced) {
            seen += names.len();
            bad.extend(
                names
                    .iter()
                    .filter(|n| !may[u].contains(*n))
                    .map(|n| format!("{tag}: classic {} was synced {n}", USERS[u])),
            );
        }
        let owner = |member: &str| {
            let mine = self
                .seats
                .iter()
                .find(|(_, s)| s.member == member)
                .map(|(u, _)| *u);
            mine.or_else(|| {
                (0..3).find(|u| self.classic[*u].as_ref().is_some_and(|c| c.id == member))
            })
        };
        for g in calls::describe_848(self.rig, &admin(), 1, &["g"], false)
            .await
            .iter()
            .filter(|g| g.error_code == 0)
        {
            for m in &g.members {
                let Some(u) = owner(&m.member_id.to_string()) else {
                    continue;
                };
                let shown = m.subscribed_topic_names.iter().map(|n| n.to_string());
                let target = m
                    .target_assignment
                    .topic_partitions
                    .iter()
                    .map(|t| t.topic_name.to_string());
                for name in shown.chain(target) {
                    seen += 1;
                    if !may[u].contains(&name) {
                        bad.push(format!(
                            "{tag}: ConsumerGroupDescribe shows {} ({}, classic {}) {name}",
                            USERS[u],
                            m.member_id,
                            m.member_type == 0
                        ));
                    }
                }
            }
        }
        seen
    }
}
