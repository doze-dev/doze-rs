//! The invariant of SEC-210 against every source of a member's topics at once: whatever the sequence of topics made,
//! DESCRIBE rights granted, denied and lifted, static members replaced by another principal, classic members joining
//! (and a classic group upgraded), no member of the group is told, shown or left a topic its own principal may not DESCRIBE.
//! After every step: every assignment every member of the new protocol was given, the partitions it owns, what each
//! classic member is given by SyncGroup, and what ConsumerGroupDescribe shows of each member (its names and its target).
//! Super users are exempt and are not among the members. (SEC-235.)
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[macro_use]
#[path = "support/authz_groups.rs"]
mod gx;
#[path = "support/authz_calls.rs"]
mod calls;
#[path = "support/classic_848.rs"]
mod classic;
#[path = "support/regex_seat.rs"]
mod seat;
#[path = "support/members_world.rs"]
mod world;

use gx::{DESCRIBE, GROUP, READ, allow, make_topic};
use seat::Seat;
use support::acls::{grant, user};
use support::{Rig, join_848};
use world::{POOL, REGEXES, Rng, SEEDS, STEPS, USERS, World, as_refs, pick, visible};

async fn start<'a>(rig: &'a Rig, r: &mut Rng, seed: u64) -> World<'a> {
    let mut w = World {
        rig,
        seats: Vec::new(),
        static_slot: None,
        classic: vec![None, None, None],
        classic_names: vec![Vec::new(); 3],
        synced: Vec::new(),
        made: Vec::new(),
        taken_over: 0,
    };
    for u in USERS {
        let who = format!("User:{u}");
        grant(
            rig,
            vec![
                allow(GROUP, "g", &who, READ),
                allow(GROUP, "g", &who, DESCRIBE),
            ],
        )
        .await;
    }
    for t in POOL.iter().take(3) {
        make_topic(rig, t, 1 + r.below(3) as i32).await;
        w.made.push(t);
    }
    for _ in 0..4 {
        w.acl_change(r).await;
    }
    // Half of the seeds begin as a classic group whose members hold what a leader gave them; a member of the new protocol upgrades it.
    if seed % 2 == 1 {
        let held = classic::assignment_bytes(&[("t0", &[0]), ("t1", &[0]), ("t2", &[0])]);
        let u = r.below(3);
        let c = classic::join(rig, &user(USERS[u]), "g", "", None, &POOL).await;
        let leader = [(c.id.as_str(), held)];
        c.sync(rig, &leader).await;
        w.classic[u] = Some(c);
        w.classic_names[u] = POOL.iter().map(|t| (*t).to_owned()).collect();
    }
    for (u, name) in USERS.iter().enumerate() {
        let may = visible(rig).await.swap_remove(u);
        let member: &'static str = ["n0", "n1", "n2"][u];
        let names = pick(r, &may);
        w.seats.push((
            u,
            Seat::start(
                rig,
                user(name),
                "g",
                member,
                None,
                join_848("g", member, &as_refs(&names)),
            )
            .await,
        ));
        let member: &'static str = ["r0", "r1", "r2"][u];
        w.seats.push((
            u,
            Seat::join_regex(rig, user(name), "g", member, REGEXES[u]).await,
        ));
    }
    w
}

#[tokio::test]
async fn random_changes_never_leave_a_member_a_topic_its_principal_cannot_describe() {
    let (mut bad, mut seen) = (Vec::new(), 0);
    for seed in 1..=SEEDS {
        let mut r = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let rig = seat::world();
        let mut w = start(&rig, &mut r, seed).await;
        // What the setup told its members was told under the rights of the time.
        w.seats.iter_mut().for_each(|(_, s)| s.fresh.clear());
        w.synced.clear();
        for step in 0..STEPS {
            match r.below(9) {
                0 => {
                    let t = POOL[r.below(POOL.len())];
                    if !w.made.contains(&t) {
                        make_topic(&rig, t, 1 + r.below(3) as i32).await;
                        w.made.push(t);
                    }
                }
                1..=3 => w.acl_change(&mut r).await,
                4 => w.takeover(&mut r).await,
                5 | 6 => w.classic_rejoin(&mut r).await,
                _ => {}
            }
            w.settle().await;
            seen += w.check(&format!("seed {seed} step {step}"), &mut bad).await;
        }
    }
    println!("{seen} observations");
    assert!(
        bad.is_empty(),
        "{} violations: {:#?}",
        bad.len(),
        &bad[..bad.len().min(10)]
    );
    assert!(seen > 20_000, "the test saw too little: {seen}");
}
