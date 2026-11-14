//! The notes of the larder's groups, oldest first.

use kafka_broker::{Cause, EventKind, GroupEvent};

use super::{Began, Held, at, began, formed, look, settled};

const P1: &str = "fulfilment-py-1";
const P2: &str = "fulfilment-py-2";
const P3: &str = "fulfilment-py-3";

/// `fulfilment`: generations 28 to 31, the newest forming (py-2 left 2.9 s ago; py-1 has 2.1 s of 5 s to join again).
pub(super) fn fulfilment() -> Vec<GroupEvent> {
    let settled_28: Held = &[
        (P1, &[("payments.settled", &[0, 1]), ("pick-lists", &[0])]),
        (P3, &[("payments.settled", &[2, 3])]),
    ];
    let settled_29: Held = &[(
        P1,
        &[("payments.settled", &[0, 1, 2, 3]), ("pick-lists", &[0])],
    )];
    let settled_30: Held = &[
        (P1, &[("payments.settled", &[0, 1]), ("pick-lists", &[0])]),
        (P2, &[("payments.settled", &[2, 3])]),
    ];
    let t28 = at("16:02:10", 0);
    let t29 = at("19:44:51", 0);
    let t30 = at("22:09:02", 0);
    vec![
        began(&Began {
            at_ms: t28,
            generation: 28,
            cause: Cause::LeaderRejoined,
            who: Some(P1),
            session_ms: None,
            members: (2, 2),
            wait_ms: 60_000,
        }),
        formed(t28 + 800, 28, &[P1, P3]),
        settled(t28 + 800, 28, settled_28),
        began(&Began {
            at_ms: t29,
            generation: 29,
            cause: Cause::SessionExpired,
            who: Some(P3),
            session_ms: Some(10_000),
            members: (2, 1),
            wait_ms: 60_000,
        }),
        formed(t29 + 300, 29, &[P1]),
        settled(t29 + 300, 29, settled_29),
        began(&Began {
            at_ms: t30,
            generation: 30,
            cause: Cause::MemberJoined,
            who: Some(P2),
            session_ms: None,
            members: (1, 2),
            wait_ms: 60_000,
        }),
        formed(t30 + 1_100, 30, &[P1, P2]),
        settled(t30 + 1_100, 30, settled_30),
        began(&Began {
            at_ms: at("22:14:19", 411),
            generation: 31,
            cause: Cause::MemberLeft,
            who: Some(P2),
            session_ms: None,
            members: (2, 1),
            wait_ms: 5_000,
        }),
    ]
}

/// `payments-workers` (KIP-848): three members joined, the last 6 h ago, epoch 14.
pub(super) fn payments_workers() -> Vec<GroupEvent> {
    let hour = 3_600_000;
    let epoch = |ago: i64, epoch: i32, who: &str, members: (u32, u32)| {
        let mut e = began(&Began {
            at_ms: look() - ago,
            generation: epoch,
            cause: Cause::MemberJoined,
            who: Some(who),
            session_ms: None,
            members,
            wait_ms: 0,
        });
        e.kind = EventKind::Epoch;
        e.wait_ms = None;
        e
    };
    vec![
        epoch(30 * hour, 12, "payments-3a", (0, 1)),
        epoch(29 * hour, 13, "payments-3b", (1, 2)),
        epoch(6 * hour, 14, "payments-3c", (2, 3)),
    ]
}

/// `search-indexer`: one generation, 3 h 44 m old: its only member joined an empty group and led it.
pub(super) fn search_indexer() -> Vec<GroupEvent> {
    let t = at("18:30:12", 0);
    let held: Held = &[(
        "search-indexer-1",
        &[
            ("clickstream", &[0, 1, 2, 3, 4, 5]),
            ("inventory.levels", &[0]),
        ],
    )];
    vec![
        began(&Began {
            at_ms: t,
            generation: 8,
            cause: Cause::MemberJoined,
            who: Some("search-indexer-1"),
            session_ms: None,
            members: (0, 1),
            wait_ms: 60_000,
        }),
        formed(t + 200, 8, &["search-indexer-1"]),
        settled(t + 200, 8, held),
    ]
}

/// A calm group's notes for a test: a world with only this.
#[must_use]
pub fn calm_world() -> Vec<GroupEvent> {
    search_indexer()
}

/// A group whose member `checkout-1` keeps timing out and coming back, as the board's illustration draws: expiry at +0:00,
/// back at +0:04, again at +0:30, +0:34, and at +1:00, +1:04, ending at `end`. (Ten minutes is the window: all are in it.)
#[must_use]
pub fn flapping_world(end: i64) -> Vec<GroupEvent> {
    let who = |n: u32| format!("checkout-{n}");
    let mut out = Vec::new();
    let mut generation = 1;
    for cycle in 0..3_i64 {
        let t = end - 180_000 + cycle * 30_000;
        let member = who(u32::try_from(cycle).unwrap_or(0));
        out.push(began(&Began {
            at_ms: t,
            generation,
            cause: Cause::SessionExpired,
            who: Some(&member),
            session_ms: Some(10_000),
            members: (1, 0),
            wait_ms: 60_000,
        }));
        generation += 1;
        let next = who(u32::try_from(cycle).unwrap_or(0) + 1);
        out.push(began(&Began {
            at_ms: t + 4_000,
            generation,
            cause: Cause::MemberJoined,
            who: Some(&next),
            session_ms: None,
            members: (0, 1),
            wait_ms: 60_000,
        }));
        generation += 1;
    }
    out
}
