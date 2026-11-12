//! The "larder" world of the group boards as raw answers (what the protocol and `ConsoleView` would say), run through the same
//! assembly the live pages use: `analytics` (empty, a commit past the end), `fulfilment` (rebalancing, one partition never
//! committed), `payments-workers` (a KIP-848 group in step) and `search-indexer` (classic, stable, 18,240 behind).
//! Used by the tests and the `boards` example; the headline numbers are checked against the boards by `tests/boards_groups.rs`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use kafka_broker::{GroupDetail, GroupKind, MemberDetail, OffsetExpiry, RebalanceDetail};

use crate::format::parse_time;
use crate::gather::group::{Cfgs, Loaded, Seen, assemble};
use crate::gather::group_read::{Commits, Described, Listed};
use crate::gather::groups::{Entry, phase_of, view};
use crate::model::{
    GroupFilter, GroupLagView, GroupMemberRow, GroupPart, GroupProtocol, GroupsView,
};

/// The moment of the world: Nov 9, 22:14:00 UTC.
#[must_use]
pub fn now() -> i64 {
    parse_time("2026-11-09 22:14:00").unwrap_or(0)
}

const DAY: i64 = 86_400_000;
const WEEK: i64 = 7 * DAY;

fn commit_day() -> i64 {
    parse_time("2026-11-06 00:14:00").unwrap_or(0)
}

/// The partitions of every topic of the world (nine topics, as the Topics tab says).
fn partitions() -> BTreeMap<String, usize> {
    [
        ("orders", 3),
        ("orders.dlq", 1),
        ("payments.settled", 4),
        ("pick-lists", 1),
        ("pick-lists.dlq", 1),
        ("inventory.levels", 1),
        ("customer.profiles", 1),
        ("notifications.email", 2),
        ("clickstream", 6),
    ]
    .into_iter()
    .map(|(t, n)| (t.to_owned(), n))
    .collect()
}

fn ends() -> HashMap<(String, i32), i64> {
    let table: [(&str, &[i64]); 5] = [
        ("orders", &[16_104, 16_051, 16_058]),
        ("payments.settled", &[3_100, 7_421, 1_960, 120]),
        ("pick-lists", &[2_310]),
        (
            "clickstream",
            &[150_519, 150_498, 150_531, 150_519, 150_527, 150_520],
        ),
        ("inventory.levels", &[61_904]),
    ];
    let mut out = HashMap::new();
    for (topic, ends) in table {
        for (p, end) in ends.iter().enumerate() {
            out.insert((topic.to_owned(), i32::try_from(p).unwrap_or(0)), *end);
        }
    }
    out
}

fn cfgs() -> Cfgs {
    [
        ("clickstream".to_owned(), (false, Some(3_600_000))),
        ("inventory.levels".to_owned(), (true, None)),
    ]
    .into()
}

fn commits(topic: &str, offsets: &[i64]) -> Commits {
    offsets
        .iter()
        .enumerate()
        .map(|(p, o)| ((topic.to_owned(), i32::try_from(p).unwrap_or(0)), *o))
        .collect()
}

fn part(topic: &str, partition: i32) -> GroupPart {
    GroupPart {
        topic: topic.to_owned(),
        partition,
    }
}

fn parts(topic: &str, n: i32) -> Vec<GroupPart> {
    (0..n).map(|p| part(topic, p)).collect()
}

#[allow(clippy::too_many_arguments)]
fn member(
    id: &str,
    client: &str,
    leader: bool,
    epoch: i32,
    session_ms: u64,
    holds: Vec<GroupPart>,
    target: Vec<GroupPart>,
) -> GroupMemberRow {
    GroupMemberRow {
        id: id.to_owned(),
        instance: None,
        client_id: client.to_owned(),
        host: "127.0.0.1".to_owned(),
        leader,
        epoch,
        session_ms,
        rebalance_ms: Some(60_000),
        heard_ago_ms: None,
        rejoined: true,
        holds,
        target,
    }
}

fn expiry(topic: &str, p: i32, committed_ms: i64, expires: Option<i64>) -> OffsetExpiry {
    OffsetExpiry {
        topic: topic.to_owned(),
        partition: p,
        committed_ms,
        expires_ms: expires,
    }
}

fn offsets_of(c: &Commits, committed_ms: i64, expires: Option<i64>) -> Vec<OffsetExpiry> {
    c.keys()
        .map(|(t, p)| expiry(t, *p, committed_ms, expires))
        .collect()
}

fn detail(id: &str, kind: GroupKind, state: &str) -> GroupDetail {
    GroupDetail {
        id: id.to_owned(),
        kind,
        state: state.to_owned(),
        protocol_type: Some("consumer".to_owned()),
        protocol: None,
        generation: 0,
        assignment_epoch: None,
        leader: None,
        rebalance: None,
        members: Vec::new(),
        empty_since_ms: None,
        offsets_retention_ms: WEEK,
        offsets: Vec::new(),
        now_ms: now(),
    }
}

fn hidden(id: &str, leader: bool, session: u64, heard: u64) -> MemberDetail {
    MemberDetail {
        id: id.to_owned(),
        instance: None,
        client_id: String::new(),
        client_host: String::new(),
        leader,
        session_ms: session,
        rebalance_ms: Some(60_000),
        heard_ago_ms: heard,
        rejoined: true,
    }
}

fn analytics() -> (Listed, Loaded) {
    let c = commits("orders", &[16_200, 16_030, 16_058]);
    let mut d = detail("analytics", GroupKind::Classic, "Empty");
    d.empty_since_ms = Some(commit_day());
    d.offsets = offsets_of(&c, commit_day(), Some(commit_day() + WEEK + 1));
    (
        Listed {
            id: "analytics".to_owned(),
            protocol: GroupProtocol::Classic,
        },
        Loaded {
            described: Some(described(GroupProtocol::Classic, "Empty", None, Vec::new())),
            detail: Some(d),
            commits: c,
        },
    )
}

fn described(
    protocol: GroupProtocol,
    state: &str,
    assignor: Option<&str>,
    members: Vec<GroupMemberRow>,
) -> Described {
    let subscribed: BTreeSet<String> = members
        .iter()
        .flat_map(|m| m.holds.iter().chain(&m.target))
        .map(|p| p.topic.clone())
        .collect();
    Described {
        protocol,
        state: state.to_owned(),
        protocol_type: Some("consumer".to_owned()),
        assignor: assignor.map(str::to_owned),
        generation: 0,
        target_epoch: None,
        members,
        subscribed,
    }
}

fn fulfilment() -> (Listed, Loaded) {
    let c = commits("payments.settled", &[3_100, 7_402, 1_958, 120]);
    let mut holds = parts("payments.settled", 2);
    holds.push(part("pick-lists", 0));
    let mut m = member(
        "fulfilment-py-1",
        "fulfilment",
        true,
        30,
        10_000,
        holds,
        Vec::new(),
    );
    m.rejoined = false;
    let mut d = detail("fulfilment", GroupKind::Classic, "PreparingRebalance");
    d.protocol = Some("range".to_owned());
    d.generation = 30;
    d.leader = Some("fulfilment-py-1".to_owned());
    d.rebalance = Some(RebalanceDetail {
        began_ago_ms: 2_900,
        remaining_ms: 2_100,
        timeout_ms: 5_000,
        waiting_for: vec!["fulfilment-py-1".to_owned()],
    });
    let mut h = hidden("fulfilment-py-1", true, 10_000, 400);
    h.rejoined = false;
    d.members = vec![h];
    d.offsets = offsets_of(&c, now() - 8_000, None);
    (
        Listed {
            id: "fulfilment".to_owned(),
            protocol: GroupProtocol::Classic,
        },
        Loaded {
            described: Some(described(
                GroupProtocol::Classic,
                "PreparingRebalance",
                Some("range"),
                vec![m],
            )),
            detail: Some(d),
            commits: c,
        },
    )
}

fn payments_workers() -> (Listed, Loaded) {
    let c = commits("orders", &[16_092, 16_039, 16_046]);
    let ids = ["payments-3a", "payments-3b", "payments-3c"];
    let members: Vec<GroupMemberRow> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let mine = vec![part("orders", i32::try_from(i).unwrap_or(0))];
            member(id, "payments", false, 14, 45_000, mine.clone(), mine)
        })
        .collect();
    let mut d = detail("payments-workers", GroupKind::Consumer, "Stable");
    d.protocol = Some("uniform".to_owned());
    d.generation = 14;
    d.assignment_epoch = Some(14);
    d.members = ids
        .iter()
        .map(|id| hidden(id, false, 45_000, 1_000))
        .collect();
    d.offsets = offsets_of(&c, now() - 3_000, None);
    let mut desc = described(GroupProtocol::Consumer, "Stable", Some("uniform"), members);
    desc.generation = 14;
    desc.target_epoch = Some(14);
    (
        Listed {
            id: "payments-workers".to_owned(),
            protocol: GroupProtocol::Consumer,
        },
        Loaded {
            described: Some(desc),
            detail: Some(d),
            commits: c,
        },
    )
}

fn search_indexer() -> (Listed, Loaded) {
    let mut c = commits(
        "clickstream",
        &[138_402, 148_594, 148_965, 149_316, 149_685, 149_912],
    );
    c.extend(commits("inventory.levels", &[61_904]));
    let mut holds = parts("clickstream", 6);
    holds.push(part("inventory.levels", 0));
    let m = member(
        "search-indexer-1",
        "search-indexer",
        true,
        8,
        45_000,
        holds,
        Vec::new(),
    );
    let mut d = detail("search-indexer", GroupKind::Classic, "Stable");
    d.protocol = Some("roundrobin".to_owned());
    d.generation = 8;
    d.leader = Some("search-indexer-1".to_owned());
    d.members = vec![hidden("search-indexer-1", true, 45_000, 1_000)];
    d.offsets = offsets_of(&c, now() - 6_000, None);
    (
        Listed {
            id: "search-indexer".to_owned(),
            protocol: GroupProtocol::Classic,
        },
        Loaded {
            described: Some(described(
                GroupProtocol::Classic,
                "Stable",
                Some("roundrobin"),
                vec![m],
            )),
            detail: Some(d),
            commits: c,
        },
    )
}

fn world() -> Vec<(Listed, Loaded)> {
    vec![
        analytics(),
        fulfilment(),
        payments_workers(),
        search_indexer(),
    ]
}

fn entries() -> Vec<Entry> {
    world()
        .into_iter()
        .map(|(listed, loaded)| Entry { listed, loaded })
        .collect()
}

/// The list. `hint` is `none` for the empty broker (the board `groups-none`).
#[must_use]
pub fn groups(filter: &str, state: GroupFilter, hint: Option<&str>) -> GroupsView {
    let all: Vec<Entry> = if hint == Some("none") {
        Vec::new()
    } else {
        entries()
    };
    let named: Vec<Entry> = all
        .iter()
        .filter(|e| filter.is_empty() || e.listed.id.contains(filter))
        .cloned()
        .collect();
    let phases: Vec<_> = named
        .iter()
        .map(|e| phase_of(&e.listed, &e.loaded).0)
        .collect();
    let shown: Vec<Entry> = named
        .into_iter()
        .filter(|e| state.takes(phase_of(&e.listed, &e.loaded).0))
        .collect();
    let mut v = view(
        filter,
        state,
        all.len(),
        &phases,
        &shown,
        false,
        &partitions(),
        &ends(),
        now(),
    );
    if hint == Some("none") {
        v.topics = 9;
    }
    v
}

/// One group with its lag; `None` for a name that is no group of the world.
#[must_use]
pub fn group(id: &str, expiring: bool) -> Option<GroupLagView> {
    let all: Vec<Listed> = world().into_iter().map(|(l, _)| l).collect();
    let (_, loaded) = world().into_iter().find(|(l, _)| l.id == id)?;
    Some(assemble(
        id,
        &loaded,
        &Seen {
            all: &all,
            counts: &partitions(),
            ends: &ends(),
            cfgs: &cfgs(),
        },
        expiring,
    ))
}

mod forms;

pub use forms::{delete_form, delete_refused, offset_delete_form, reset, reset_refused};
