use super::*;

fn id(n: u8) -> MemberId {
    MemberId::new(&format!("m{n}")).unwrap()
}

fn topic(n: u128, partitions: i32, subscribers: &[u8]) -> Subscribed {
    Subscribed {
        id: Uuid::from_u128(n),
        partitions,
        subscribers: subscribers.iter().map(|s| id(*s)).collect(),
    }
}

fn members(n: u8) -> Vec<MemberId> {
    (1..=n).map(id).collect()
}

fn sizes(t: &BTreeMap<MemberId, Partitions>) -> Vec<usize> {
    t.values().map(BTreeSet::len).collect()
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) % n
    }
}

#[test]
fn range_gives_each_subscriber_a_run_and_the_first_ones_the_extra() {
    let topics = [topic(1, 5, &[1, 2])];
    let t = assign(Assignor::Range, &members(2), &topics, &BTreeMap::new());
    let of = |n: u8| -> Vec<i32> { t[&id(n)].iter().map(|p| p.1).collect() };
    assert_eq!((of(1), of(2)), (vec![0, 1, 2], vec![3, 4]));
}

#[test]
fn range_deals_topic_by_topic_so_equal_topics_line_up() {
    let topics = [topic(1, 4, &[1, 2]), topic(2, 4, &[1, 2])];
    let t = assign(Assignor::Range, &members(2), &topics, &BTreeMap::new());
    let mine: Vec<_> = t[&id(1)].iter().map(|p| (p.0.as_u128(), p.1)).collect();
    assert_eq!(mine, [(1, 0), (1, 1), (2, 0), (2, 1)]);
}

#[test]
fn uniform_spreads_a_topic_evenly_and_a_lone_member_gets_everything() {
    let topics = [topic(1, 4, &[1, 2, 3])];
    let t = assign(Assignor::Uniform, &members(3), &topics, &BTreeMap::new());
    let mut s = sizes(&t);
    s.sort_unstable();
    assert_eq!(s, [1, 1, 2]);
    let alone = assign(
        Assignor::Uniform,
        &members(1),
        &[topic(1, 3, &[1])],
        &BTreeMap::new(),
    );
    assert_eq!(alone[&id(1)].len(), 3);
}

#[test]
fn uniform_balances_across_topics_not_topic_by_topic() {
    // Six single-partition topics and one of three: three members get three each, not 7, 1 and 1.
    let mut topics: Vec<_> = (1..=6).map(|n| topic(n, 1, &[1, 2, 3])).collect();
    topics.push(topic(7, 3, &[1, 2, 3]));
    let t = assign(Assignor::Uniform, &members(3), &topics, &BTreeMap::new());
    assert_eq!(sizes(&t), [3, 3, 3]);
}

#[test]
fn uniform_keeps_what_a_member_had_when_another_joins() {
    let topics = [topic(1, 6, &[1, 2])];
    let before = assign(Assignor::Uniform, &members(2), &topics, &BTreeMap::new());
    let topics = [topic(1, 6, &[1, 2, 3])];
    let after = assign(Assignor::Uniform, &members(3), &topics, &before);
    assert_eq!(sizes(&after), [2, 2, 2]);
    // Nothing moved between the two who were there, only to the newcomer.
    for m in [1, 2] {
        assert!(
            after[&id(m)].is_subset(&before[&id(m)]),
            "m{m} gave up only what the newcomer took"
        );
    }
}

#[test]
fn uniform_leaves_a_partition_with_nobody_subscribed_to_it_unowned_and_respects_subscriptions() {
    let topics = [topic(1, 2, &[1]), topic(2, 2, &[2]), topic(3, 2, &[])];
    let t = assign(Assignor::Uniform, &members(2), &topics, &BTreeMap::new());
    assert!(t[&id(1)].iter().all(|p| p.0 == Uuid::from_u128(1)));
    assert!(t[&id(2)].iter().all(|p| p.0 == Uuid::from_u128(2)));
    assert_eq!(sizes(&t), [2, 2]);
}

/// Random subscriptions: every partition with a subscriber goes to exactly one subscriber; for `uniform`, each
/// topic's subscribers hold counts within one of each other when they all subscribe to the same topics.
#[test]
fn any_subscriptions_give_every_partition_one_eligible_owner() {
    for seed in 1..=300 {
        let mut rng = Rng(seed);
        let n_members = 1 + rng.below(6) as u8;
        let all = members(n_members);
        let topics: Vec<Subscribed> = (1..=1 + rng.below(5))
            .map(|t| {
                let subs: Vec<u8> = (1..=n_members).filter(|_| rng.below(3) != 0).collect();
                topic(u128::from(t), rng.below(8) as i32, &subs)
            })
            .collect();
        // A previous assignment that is partly stale.
        let mut previous = BTreeMap::new();
        for m in &all {
            let held: Partitions = (0..rng.below(4))
                .map(|_| {
                    (
                        Uuid::from_u128(1 + u128::from(rng.below(6))),
                        rng.below(8) as i32,
                    )
                })
                .collect();
            previous.insert(m.clone(), held);
        }
        for assignor in [Assignor::Uniform, Assignor::Range] {
            let t = assign(assignor, &all, &topics, &previous);
            let mut seen = Partitions::new();
            for (m, held) in &t {
                for p in held {
                    let topic = topics
                        .iter()
                        .find(|x| x.id == p.0)
                        .expect("a topic that exists");
                    assert!(
                        p.1 >= 0 && p.1 < topic.partitions,
                        "seed {seed}: {p:?} exists"
                    );
                    assert!(
                        topic.subscribers.contains(m),
                        "seed {seed}: {m:?} subscribes"
                    );
                    assert!(seen.insert(*p), "seed {seed}: {p:?} is owned twice");
                }
            }
            for topic in topics.iter().filter(|t| !t.subscribers.is_empty()) {
                for p in 0..topic.partitions {
                    assert!(
                        seen.contains(&(topic.id, p)),
                        "seed {seed}: {assignor:?} left {p} unowned"
                    );
                }
            }
        }
    }
}

#[test]
fn uniform_is_balanced_within_one_when_everyone_subscribes_to_everything() {
    for seed in 1..=300 {
        let mut rng = Rng(seed);
        let n = 1 + rng.below(7) as u8;
        let all = members(n);
        let subs: Vec<u8> = (1..=n).collect();
        let topics: Vec<Subscribed> = (1..=1 + rng.below(4))
            .map(|t| topic(u128::from(t), 1 + rng.below(9) as i32, &subs))
            .collect();
        let t = assign(Assignor::Uniform, &all, &topics, &BTreeMap::new());
        let s = sizes(&t);
        let (lo, hi) = (s.iter().min().unwrap(), s.iter().max().unwrap());
        assert!(hi - lo <= 1, "seed {seed}: {s:?}");
    }
}
