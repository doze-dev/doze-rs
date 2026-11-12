//! Authorization of OffsetCommit, OffsetFetch and OffsetDelete. No row is recorded: each test cites the Kafka 4.3.1
//! source it is written from (`core/src/main/scala/kafka/server/KafkaApis.scala`, "KA" below).
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

use gx::{
    DELETE, DESCRIBE, GROUP, GROUP_AUTH, READ, TOPIC, TOPIC_AUTH, allow, deny, make_topic, need,
    world,
};
use support::acls::{admin, grant, user};

/// Two groups with an offset each on topics `t` (3 partitions) and `u`.
async fn committed(rig: &support::Rig) {
    make_topic(rig, "t", 3).await;
    make_topic(rig, "u", 1).await;
    for g in ["grp", "other"] {
        let r = calls::commit(rig, &admin(), 8, g, &[("t", 0, 7), ("u", 0, 9)]).await;
        assert!(r.topics.iter().all(|t| t.partitions[0].error_code == 0));
    }
}

// ---- OffsetCommit (KA:273-374: READ on the group, then READ on each topic, before the topic is looked for)

#[tokio::test]
async fn offset_commit_needs_read_on_the_group_and_on_the_topic() {
    five_ways!(
        &[
            need(GROUP, "grp", READ, GROUP_AUTH),
            need(TOPIC, "tpc", READ, TOPIC_AUTH)
        ],
        |rig, who| {
            make_topic(rig, "tpc", 1).await;
            let r = calls::commit(rig, who, 8, "grp", &[("tpc", 0, 3)]).await;
            calls::first_commit_code(&r)
        }
    );
}

#[tokio::test]
async fn a_refused_group_fails_every_partition_of_every_topic_in_each_version() {
    // OffsetCommitRequest.getErrorResponse (KA:278-281): every partition, topics and ids as sent.
    let rig = world();
    committed(&rig).await;
    grant(&rig, vec![allow(TOPIC, "t", "User:bob", READ)]).await;
    for version in 2..=8 {
        let r = calls::commit(
            &rig,
            &user("bob"),
            version,
            "grp",
            &[("t", 0, 1), ("t", 2, 1), ("u", 0, 1)],
        )
        .await;
        let seen: Vec<_> = r
            .topics
            .iter()
            .map(|t| {
                (
                    t.name.as_str(),
                    t.partitions
                        .iter()
                        .map(|p| p.error_code)
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        assert_eq!(seen, [("t", vec![30, 30]), ("u", vec![30])], "v{version}");
    }
}

#[tokio::test]
async fn a_refused_topic_fails_its_partitions_and_the_others_are_committed() {
    let rig = world();
    committed(&rig).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "grp", "User:alice", READ),
            allow(TOPIC, "t", "User:alice", READ),
            deny(TOPIC, "u", "User:alice", READ),
            allow(TOPIC, "u", "User:alice", READ),
        ],
    )
    .await;
    let r = calls::commit(
        &rig,
        &user("alice"),
        8,
        "grp",
        &[("t", 1, 40), ("u", 0, 41)],
    )
    .await;
    let seen: Vec<_> = r
        .topics
        .iter()
        .map(|t| (t.name.as_str(), t.partitions[0].error_code))
        .collect();
    assert_eq!(seen, [("t", 0), ("u", 29)]);
    // t was stored; u was not (still 9).
    let got = calls::fetch(&rig, &admin(), 8, "grp", None).await;
    let offsets: Vec<_> = got.groups[0]
        .topics
        .iter()
        .flat_map(|t| {
            t.partitions
                .iter()
                .map(|p| (t.name.to_string(), p.partition_index, p.committed_offset))
        })
        .collect();
    assert!(offsets.contains(&("t".to_owned(), 1, 40)), "{offsets:?}");
    assert!(offsets.contains(&("u".to_owned(), 0, 9)), "{offsets:?}");
}

#[tokio::test]
async fn offset_commit_checks_the_group_before_the_topics() {
    // Neither right: the group's 30 wins on every partition, the topic's 29 is never seen (KA:278 before 292).
    let rig = world();
    committed(&rig).await;
    let r = calls::commit(&rig, &user("bob"), 8, "grp", &[("t", 0, 1)]).await;
    assert_eq!(calls::first_commit_code(&r), 30);
}

#[tokio::test]
async fn a_topic_that_is_not_there_is_not_revealed_to_one_who_may_not_read_it() {
    // KA:292-299 authorizes the name before the metadata cache is asked: 29 for a missing topic, as for a present one;
    // a caller that may read it is told it is unknown (3).
    let rig = world();
    grant(
        &rig,
        vec![
            allow(GROUP, "grp", "User:alice", READ),
            allow(GROUP, "grp", "User:bob", READ),
            allow(TOPIC, "missing", "User:bob", READ),
        ],
    )
    .await;
    let code = |who: &str| {
        let (rig, who) = (&rig, user(who));
        async move {
            calls::first_commit_code(
                &calls::commit(rig, &who, 8, "grp", &[("missing", 0, 1)]).await,
            )
        }
    };
    assert_eq!(code("alice").await, 29);
    assert_eq!(code("bob").await, 3);
}

// ---- OffsetFetch (KA:1018-1185: DESCRIBE on the group, then DESCRIBE on each topic)

#[tokio::test]
async fn offset_fetch_needs_describe_on_the_group_and_the_topic() {
    for version in [7_i16, 8] {
        five_ways!(
            &[
                need(GROUP, "grp", DESCRIBE, GROUP_AUTH),
                need(TOPIC, "tpc", DESCRIBE, TOPIC_AUTH)
            ],
            |rig, who| {
                make_topic(rig, "tpc", 1).await;
                let r = calls::fetch(rig, who, version, "grp", Some(&[("tpc", &[0])])).await;
                if version >= 8 {
                    let g = &r.groups[0];
                    if g.error_code != 0 {
                        g.error_code
                    } else {
                        g.topics[0].partitions[0].error_code
                    }
                } else if r.error_code != 0 {
                    r.error_code
                } else {
                    r.topics[0].partitions[0].error_code
                }
            }
        );
    }
}

#[tokio::test]
async fn a_refused_group_is_answered_in_the_shape_of_the_version() {
    // OffsetFetchResponse.groupError (OffsetFetchResponse.java:229-254): v8 and later in the group's entry, v2 to v7 at
    // the top, v1 (no code at the top) on each partition asked for with offset -1.
    let rig = world();
    committed(&rig).await;
    let bob = user("bob");
    let r = calls::fetch(&rig, &bob, 9, "grp", Some(&[("t", &[0])])).await;
    assert_eq!(
        (r.groups[0].group_id.as_str(), r.groups[0].error_code),
        ("grp", 30)
    );
    assert!(r.groups[0].topics.is_empty());
    for version in 2..=7 {
        let r = calls::fetch(&rig, &bob, version, "grp", Some(&[("t", &[0])])).await;
        assert_eq!(r.error_code, 30, "v{version}");
        assert!(r.topics.is_empty(), "v{version}");
    }
    let r = calls::fetch(&rig, &bob, 1, "grp", Some(&[("t", &[0, 2]), ("u", &[0])])).await;
    let seen: Vec<_> = r
        .topics
        .iter()
        .flat_map(|t| {
            t.partitions.iter().map(|p| {
                (
                    t.name.to_string(),
                    p.partition_index,
                    p.error_code,
                    p.committed_offset,
                )
            })
        })
        .collect();
    assert_eq!(
        seen,
        [
            ("t".to_owned(), 0, 30, -1),
            ("t".to_owned(), 2, 30, -1),
            ("u".to_owned(), 0, 30, -1)
        ]
    );
}

#[tokio::test]
async fn a_refused_topic_asked_for_by_name_is_29_with_offset_minus_one_after_the_others() {
    // KA:1124-1185: the error topics follow the found ones, each partition 29 and -1.
    let rig = world();
    committed(&rig).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "grp", "User:alice", DESCRIBE),
            allow(TOPIC, "u", "User:alice", DESCRIBE),
        ],
    )
    .await;
    for version in [7_i16, 8] {
        let r = calls::fetch(
            &rig,
            &user("alice"),
            version,
            "grp",
            Some(&[("t", &[0, 1]), ("u", &[0])]),
        )
        .await;
        let topics: Vec<(String, Vec<(i16, i64)>)> = if version >= 8 {
            r.groups[0]
                .topics
                .iter()
                .map(|t| {
                    (
                        t.name.to_string(),
                        t.partitions
                            .iter()
                            .map(|p| (p.error_code, p.committed_offset))
                            .collect(),
                    )
                })
                .collect()
        } else {
            r.topics
                .iter()
                .map(|t| {
                    (
                        t.name.to_string(),
                        t.partitions
                            .iter()
                            .map(|p| (p.error_code, p.committed_offset))
                            .collect(),
                    )
                })
                .collect()
        };
        assert_eq!(
            topics,
            [
                ("u".to_owned(), vec![(0, 9)]),
                ("t".to_owned(), vec![(29, -1), (29, -1)])
            ],
            "v{version}"
        );
    }
}

#[tokio::test]
async fn asked_for_everything_the_topics_not_described_are_dropped_without_a_word() {
    // KA:1090-1123: the result is filtered by DESCRIBE; nothing says there is more.
    let rig = world();
    committed(&rig).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "grp", "User:alice", DESCRIBE),
            allow(TOPIC, "u", "User:alice", DESCRIBE),
        ],
    )
    .await;
    for version in [7_i16, 8] {
        let r = calls::fetch(&rig, &user("alice"), version, "grp", None).await;
        let names: Vec<String> = if version >= 8 {
            r.groups[0]
                .topics
                .iter()
                .map(|t| t.name.to_string())
                .collect()
        } else {
            r.topics.iter().map(|t| t.name.to_string()).collect()
        };
        assert_eq!(names, ["u"], "v{version}");
    }
}

#[tokio::test]
async fn a_topic_that_is_not_there_is_not_revealed_to_offset_fetch_either() {
    let rig = world();
    grant(
        &rig,
        vec![
            allow(GROUP, "grp", "User:alice", DESCRIBE),
            allow(GROUP, "grp", "User:bob", DESCRIBE),
            allow(TOPIC, "missing", "User:bob", DESCRIBE),
        ],
    )
    .await;
    let code = |who: &str| {
        let (rig, who) = (&rig, user(who));
        async move {
            calls::fetch(rig, &who, 7, "grp", Some(&[("missing", &[0])]))
                .await
                .topics[0]
                .partitions[0]
                .error_code
        }
    };
    assert_eq!(code("alice").await, 29);
    // A caller that may describe it is told what it would be told of a topic with nothing committed: -1, no error.
    assert_eq!(code("bob").await, 0);
}

// ---- OffsetDelete (KA:2363-2427: DELETE on the group, then READ on each topic)

#[tokio::test]
async fn offset_delete_needs_delete_on_the_group_and_read_on_the_topic() {
    five_ways!(
        &[
            need(GROUP, "grp", DELETE, GROUP_AUTH),
            need(TOPIC, "tpc", READ, TOPIC_AUTH)
        ],
        |rig, who| {
            make_topic(rig, "tpc", 1).await;
            calls::commit(rig, &admin(), 8, "grp", &[("tpc", 0, 3)]).await;
            let r = calls::offset_delete(rig, who, "grp", &[("tpc", 0)]).await;
            let (top, parts) = calls::delete_codes(&r);
            if top != 0 { top } else { parts[0] }
        }
    );
}

#[tokio::test]
async fn offset_delete_refuses_the_group_at_the_top_and_a_topic_on_its_partitions() {
    let rig = world();
    committed(&rig).await;
    let bob = user("bob");
    let r = calls::offset_delete(&rig, &bob, "grp", &[("t", 0)]).await;
    assert_eq!(calls::delete_codes(&r), (30, vec![]));
    grant(
        &rig,
        vec![
            allow(GROUP, "grp", "User:alice", DELETE),
            allow(TOPIC, "t", "User:alice", READ),
        ],
    )
    .await;
    let r =
        calls::offset_delete(&rig, &user("alice"), "grp", &[("t", 0), ("u", 0), ("t", 1)]).await;
    // t is deleted (0), u is refused (29); the answer lists the topics in the order asked.
    assert_eq!(calls::delete_codes(&r), (0, vec![0, 0, 29]));
    // Nothing of u was deleted.
    let got = calls::fetch(&rig, &admin(), 7, "grp", Some(&[("u", &[0])])).await;
    assert_eq!(got.topics[0].partitions[0].committed_offset, 9);
}

#[tokio::test]
async fn offset_delete_does_not_reveal_a_topic_or_a_group_either() {
    // The group is authorized before it is looked for (KA:2368): a group that is not there is 30 to one who may not
    // delete it, and GROUP_ID_NOT_FOUND (69) to one who may. The topic likewise (KA:2386-2392).
    let rig = world();
    grant(&rig, vec![allow(GROUP, "nowhere", "User:alice", DELETE)]).await;
    let r = calls::offset_delete(&rig, &user("bob"), "nowhere", &[("t", 0)]).await;
    assert_eq!(r.error_code, 30);
    let r = calls::offset_delete(&rig, &user("alice"), "nowhere", &[("t", 0)]).await;
    assert_eq!(r.error_code, 69);
}
