//! The Story's pure parts: words, generations made of notes, and the flapping rule, each by a table of cases.

use kafka_broker::{Cause, GroupEvent};

use super::*;
use crate::fixtures_story::{Began, at, began, events, flapping_world, formed, settled};
use crate::model::{GroupProtocol, StoryCause, StoryGen, StoryTone};

fn gens(id: &str) -> Vec<StoryGen> {
    let e = events(id).unwrap();
    generations_of(&e, GroupProtocol::Classic, e.now_ms)
}

fn world_flap(end: i64, evs: Vec<GroupEvent>) -> crate::model::StoryFlap {
    let all = kafka_broker::GroupEvents {
        group: "g".into(),
        now_ms: end,
        events: evs,
        events_per_group: 200,
        groups_kept: 100,
        ring_bytes: 65_536,
    };
    let mut g = generations_of(&all, GroupProtocol::Classic, end);
    g.reverse();
    flap_of(&g, end)
}

#[test]
fn a_member_id_loses_the_unique_tail_the_broker_adds_and_nothing_else() {
    let uuid = "7b0f6a0e-1c2d-4e3f-8a9b-0c1d2e3f4a5b";
    for (id, shown) in [
        ("fulfilment-py-1", "fulfilment-py-1"),
        (
            "fulfilment-py-1-7b0f6a0e-1c2d-4e3f-8a9b-0c1d2e3f4a5b",
            "fulfilment-py-1",
        ),
        (uuid, uuid),
        (
            "-7b0f6a0e-1c2d-4e3f-8a9b-0c1d2e3f4a5b",
            "-7b0f6a0e-1c2d-4e3f-8a9b-0c1d2e3f4a5b",
        ),
        (
            "a-7b0f6a0e-1c2d-4e3f-8a9b-0c1d2e3f4a5z",
            "a-7b0f6a0e-1c2d-4e3f-8a9b-0c1d2e3f4a5z",
        ),
        ("é", "é"),
        ("", ""),
    ] {
        assert_eq!(member_label(id), shown, "{id}");
    }
}

#[test]
fn partitions_are_said_as_the_boards_say_them() {
    let one = |t: &str, ps: &[i32]| (t.to_owned(), ps.to_vec());
    for (held, said) in [
        (
            vec![one("payments.settled", &[0, 1]), one("pick-lists", &[0])],
            "payments.settled-0, -1 and pick-lists-0",
        ),
        (
            vec![
                one("clickstream", &[0, 1, 2, 3, 4, 5]),
                one("inventory.levels", &[0]),
            ],
            "clickstream-0 to 5 and inventory.levels-0",
        ),
        (vec![one("t", &[3, 1, 2])], "t-1 to 3"),
        (vec![one("a", &[0, 2])], "a-0 and -2"),
        (vec![one("a", &[0, 0, 1])], "a-0 and -1"),
        (vec![one("a", &[5])], "a-5"),
        (vec![], ""),
    ] {
        assert_eq!(parts_phrase(&held), said);
    }
    assert_eq!(
        holds_phrase("fulfilment-py-1", &[]),
        "fulfilment-py-1 held nothing"
    );
}

#[test]
fn how_long_a_generation_ran_is_said_in_the_boards_units() {
    for (ms, said) in [
        (0, "0 s"),
        (1_100, "1.1 s"),
        (59_900, "59.9 s"),
        (60_000, "1 m 0 s"),
        (317_411, "5 m 17 s"),
        (3_599_000, "59 m 59 s"),
        (8_640_000, "2 h 24 m"),
        (13_320_000, "3 h 42 m"),
        (86_400_000, "1 d 0 h"),
        (100 * 3_600_000, "4 d 4 h"),
    ] {
        assert_eq!(lasted(ms), said, "{ms}");
    }
    assert_eq!(ago_near(2_900), "3 s ago");
    assert_eq!(ago_near(400), "0.4 s ago");
    assert_eq!(ago_near(6 * 3_600_000), "6 h ago");
}

#[test]
fn the_ring_of_a_rebalancing_group_is_four_generations_newest_first_with_what_came_of_each() {
    let g = gens("fulfilment");
    let seen: Vec<_> = g
        .iter()
        .map(|g| (g.generation, g.cause, g.forming))
        .collect();
    assert_eq!(
        seen,
        [
            (31, StoryCause::MemberLeft, true),
            (30, StoryCause::MemberJoined, false),
            (29, StoryCause::SessionExpired, false),
            (28, StoryCause::LeaderRejoined, false),
        ]
    );
    assert_eq!(g[1].settled_ms, Some(1_100));
    assert_eq!(
        g[1].lasted_ms,
        Some(317_411),
        "from 22:09:02.000 to 22:14:19.411: 5 m 17 s"
    );
    assert_eq!(g[0].lasted_ms, Some(2_900), "the newest, so far");
    assert_eq!(g[2].who.as_ref().unwrap().session_ms, Some(10_000));
    assert_eq!((g[2].before, g[2].after), (2, 1));
    assert_eq!(g[1].tone, StoryTone::Plain);
    assert_eq!(g[2].tone, StoryTone::Bad);
    assert_eq!(g[0].tone, StoryTone::Warn);
    // What moves when a member leaves is what it held in the generation before.
    assert_eq!(
        g[0].moves,
        [(
            "fulfilment-py-2".to_owned(),
            vec![("payments.settled".to_owned(), vec![2, 3])]
        )]
    );
    assert_eq!(g[2].moves[0].0, "fulfilment-py-3");
    assert!(
        g[1].moves.is_empty(),
        "a member that joins moves nothing out"
    );
}

#[test]
fn notes_about_a_generation_whose_beginning_was_let_go_are_not_drawn() {
    let mut e = events("fulfilment").unwrap();
    e.events.drain(..1); // the Began of generation 28; its Formed and Settled stay
    let g = generations_of(&e, GroupProtocol::Classic, e.now_ms);
    assert_eq!(g.len(), 3);
    assert_eq!(g.last().unwrap().generation, 29);
}

#[test]
fn a_member_dropped_as_the_generation_forms_is_told_with_how_long_it_was_waited_for() {
    let t = at("10:00:00", 0);
    let mut dropped = crate::fixtures_story::blank(kafka_broker::EventKind::Dropped, t + 30_000, 2);
    dropped.member = Some("m2".into());
    dropped.cause = Some(Cause::RebalanceTimeout);
    dropped.wait_ms = Some(30_000);
    let ring = kafka_broker::GroupEvents {
        group: "g".into(),
        now_ms: t + 40_000,
        events: vec![
            began(&Began {
                at_ms: t,
                generation: 2,
                cause: Cause::MemberLeft,
                who: Some("m3"),
                session_ms: None,
                members: (3, 2),
                wait_ms: 30_000,
            }),
            dropped,
            formed(t + 30_000, 2, &["m1"]),
            settled(t + 30_100, 2, &[("m1", &[("t", &[0, 1])])]),
        ],
        events_per_group: 200,
        groups_kept: 100,
        ring_bytes: 1,
    };
    let g = generations_of(&ring, GroupProtocol::Classic, ring.now_ms);
    assert_eq!(g.len(), 1);
    assert_eq!((g[0].dropped.len(), g[0].dropped[0].waited_ms), (1, 30_000));
    assert_eq!((g[0].after, g[0].members_total), (1, 1));
}

#[test]
fn the_tabs_and_sentences_know_how_many_and_the_newest() {
    let e = events("fulfilment").unwrap();
    let f = facts_of(Some(&e), GroupProtocol::Classic);
    assert_eq!(f.count, 4);
    let last = f.last.unwrap();
    assert_eq!(
        (last.cause, last.who.as_deref(), last.ago_ms, last.forming),
        (StoryCause::MemberLeft, Some("fulfilment-py-2"), 2_900, true)
    );
    assert_eq!(facts_of(None, GroupProtocol::Classic).count, 0);
    let c = causes_of(&gens("fulfilment"));
    assert_eq!(c.len(), 4);
    assert!(c.iter().all(|(_, n)| *n == 1));
}

#[test]
fn a_group_that_rebalanced_once_or_twice_and_is_not_forming_is_calm_and_the_rest_are_not() {
    assert!(calm_of("search-indexer", &gens("search-indexer")).is_some());
    assert!(
        calm_of("fulfilment", &gens("fulfilment")).is_none(),
        "four, and one forming"
    );
    assert!(calm_of("none", &[]).is_none());
}

fn at_end(end: i64, ago_ms: i64) -> i64 {
    end - ago_ms
}

fn cycle_world(end: i64, expiry_ago: &[i64], joined_after: &[i64], who: &str) -> Vec<GroupEvent> {
    let mut out: Vec<GroupEvent> = Vec::new();
    let mut n = 0;
    for (ago, back) in expiry_ago.iter().zip(joined_after) {
        n += 1;
        out.push(began(&Began {
            at_ms: at_end(end, *ago),
            generation: n,
            cause: Cause::SessionExpired,
            who: Some(&format!("{who}-{n}")),
            session_ms: Some(10_000),
            members: (1, 0),
            wait_ms: 1,
        }));
        n += 1;
        out.push(began(&Began {
            at_ms: at_end(end, *ago) + back,
            generation: n,
            cause: Cause::MemberJoined,
            who: Some(&format!("{who}-{n}")),
            session_ms: None,
            members: (0, 1),
            wait_ms: 1,
        }));
    }
    out
}

#[test]
fn three_expired_then_joined_cycles_in_ten_minutes_are_flapping_and_two_are_not() {
    let end = at("12:00:00", 0);
    let f = world_flap(end, flapping_world(end));
    assert_eq!((f.flapping, f.cycles), (true, 3));
    let two: Vec<GroupEvent> = flapping_world(end).into_iter().skip(2).collect();
    let f = world_flap(end, two);
    assert_eq!((f.flapping, f.cycles), (false, 2));
}

#[test]
fn the_window_is_ten_minutes_and_its_edge_counts() {
    let end = at("12:00:00", 0);
    let w = i64::try_from(WINDOW_MS).unwrap();
    // Three cycles whose expiries are exactly 10 minutes ago, 9 and 8: all inside.
    let inside = cycle_world(end, &[w, w - 60_000, w - 120_000], &[4_000; 3], "a");
    assert_eq!(world_flap(end, inside).cycles, 3);
    // The first one a millisecond older than the window: it does not count.
    let outside = cycle_world(end, &[w + 1, w - 60_000, w - 120_000], &[4_000; 3], "a");
    let f = world_flap(end, outside);
    assert_eq!((f.cycles, f.flapping), (2, false));
}

#[test]
fn a_clean_leave_never_counts_and_a_join_after_a_leave_is_no_cycle() {
    let end = at("12:00:00", 0);
    let mut evs = Vec::new();
    for n in 0..5 {
        let t = end - 300_000 + i64::from(n) * 10_000;
        evs.push(began(&Began {
            at_ms: t,
            generation: n * 2,
            cause: Cause::MemberLeft,
            who: Some(&format!("c-{n}")),
            session_ms: None,
            members: (1, 0),
            wait_ms: 1,
        }));
        evs.push(began(&Began {
            at_ms: t + 1_000,
            generation: n * 2 + 1,
            cause: Cause::MemberJoined,
            who: Some(&format!("c-{}", n + 1)),
            session_ms: None,
            members: (0, 1),
            wait_ms: 1,
        }));
    }
    let f = world_flap(end, evs);
    assert_eq!((f.cycles, f.flapping), (0, false));
}

#[test]
fn the_member_that_comes_back_is_the_client_that_expired() {
    let end = at("12:00:00", 0);
    // Three expiries of client x, and three joins of client y: nobody came back.
    let mut evs = cycle_world(end, &[300_000, 200_000, 100_000], &[4_000; 3], "x");
    for e in &mut evs {
        if e.cause == Some(Cause::MemberJoined) {
            e.client = Some("y".into());
        }
    }
    assert_eq!(world_flap(end, evs).cycles, 0);
    // One expiry, then three joins of the same client: one cycle, as an expiry is used once.
    let mut evs = cycle_world(end, &[300_000], &[4_000], "x");
    for n in 0..2 {
        evs.push(began(&Began {
            at_ms: end - 100_000 + n * 1_000,
            generation: 10 + i32::try_from(n).unwrap(),
            cause: Cause::MemberJoined,
            who: Some("x-9"),
            session_ms: None,
            members: (0, 1),
            wait_ms: 1,
        }));
    }
    assert_eq!(world_flap(end, evs).cycles, 1);
}

#[test]
fn the_rule_held_against_the_boards_group_says_what_the_board_says() {
    let e = events("fulfilment").unwrap();
    let mut g = generations_of(&e, GroupProtocol::Classic, e.now_ms);
    g.reverse();
    let f = flap_of(&g, e.now_ms);
    assert_eq!(
        (f.flapping, f.cycles, f.expiries, f.expiries_in_window),
        (false, 0, 1, 0)
    );
    let rows: Vec<_> = f
        .rows
        .iter()
        .map(|r| (r.ok, r.title.as_str(), r.value.as_str()))
        .collect();
    assert_eq!(
        rows[0],
        (
            true,
            "Expired, then joined, 3 times in 10 minutes",
            "0 cycles"
        )
    );
    assert_eq!(rows[1], (true, "A leave is not a timeout", "not counted"));
    assert_eq!(rows[2].2, "1");
    assert_eq!(f.rows[2].detail, "fulfilment-py-3 at 19:44:51 only.");
    assert_eq!(
        rows[3],
        (
            false,
            "A member joined and left within 6 minutes",
            "5 m 17 s"
        )
    );
    assert!(
        f.rows[0].detail.starts_with(
            "Last 10 minutes: 1 join (22:09:02), 1 clean leave (22:14:19), 0 expiries"
        ),
        "{}",
        f.rows[0].detail
    );
}
