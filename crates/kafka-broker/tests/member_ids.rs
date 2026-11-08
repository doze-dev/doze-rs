//! Member ids handed to clients that have not yet come back with them (JoinGroup v4 and later).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use support::{join_request, rig_with};

const NONE: i16 = 0;
const UNKNOWN_MEMBER_ID: i16 = 25;
const MEMBER_ID_REQUIRED: i16 = 79;

fn rig_after(delay: Duration) -> support::Rig {
    rig_with(|c| c.group_initial_rebalance_delay = delay)
}

#[tokio::test(start_paused = true)]
async fn an_issued_id_is_used_up_by_a_join_that_is_taken_and_not_by_one_that_is_refused() {
    let rig = rig_after(Duration::ZERO);
    // A group with a member, so that a join can be refused for having nothing in common with it.
    let original = rig
        .join(3, join_request("g", "", None, &[("range", "m")]))
        .await
        .member_id
        .to_string();
    let told = rig
        .join(4, join_request("g", "", None, &[("range", "m")]))
        .await;
    assert_eq!(told.error_code, MEMBER_ID_REQUIRED);
    let id = told.member_id.as_str().to_owned();
    let refused = rig
        .join(4, join_request("g", &id, None, &[("sticky", "m")]))
        .await;
    assert_eq!(refused.error_code, 23, "nothing in common with the group");
    // Not used up: the same id is good for a join that is taken (it waits for the first member to join again).
    let (taken, rejoined) = tokio::join!(
        rig.join(4, join_request("g", &id, None, &[("range", "m")])),
        async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            rig.join(4, join_request("g", &original, None, &[("range", "m")]))
                .await
        }
    );
    assert_eq!((taken.error_code, rejoined.error_code), (NONE, NONE));
    // Used up now: after leaving, the id is no member's and nobody's to come back with.
    rig.leave(4, "g", &[&id]).await;
    let again = rig
        .join(4, join_request("g", &id, None, &[("range", "m")]))
        .await;
    assert_eq!(again.error_code, UNKNOWN_MEMBER_ID);
}

#[tokio::test(start_paused = true)]
async fn an_issued_id_is_forgotten_after_five_minutes() {
    let rig = rig_after(Duration::ZERO);
    let told = rig
        .join(4, join_request("g", "", None, &[("range", "m")]))
        .await;
    let id = told.member_id.as_str().to_owned();
    tokio::time::sleep(Duration::from_secs(299)).await;
    let early = rig
        .join(4, join_request("g", &id, None, &[("range", "m")]))
        .await;
    assert_eq!(
        early.error_code, NONE,
        "still good at four minutes fifty-nine"
    );
    rig.leave(4, "g", &[&id]).await;
    let told = rig
        .join(4, join_request("g2", "", None, &[("range", "m")]))
        .await;
    let id = told.member_id.as_str().to_owned();
    tokio::time::sleep(Duration::from_secs(301)).await;
    let late = rig
        .join(4, join_request("g2", &id, None, &[("range", "m")]))
        .await;
    assert_eq!(late.error_code, UNKNOWN_MEMBER_ID);
}

#[tokio::test]
async fn a_client_that_only_ever_asks_for_ids_leaves_no_group_behind() {
    let rig = rig_after(Duration::ZERO);
    for n in 0..300 {
        let told = rig
            .join(
                4,
                join_request(&format!("ghost{n}"), "", None, &[("range", "m")]),
            )
            .await;
        assert_eq!(told.error_code, MEMBER_ID_REQUIRED);
    }
    assert!(
        rig.list_groups(&[], &[]).await.is_empty(),
        "no group, in memory or recorded"
    );
}

#[tokio::test]
async fn a_join_with_an_id_nobody_gave_it_is_told_so_and_makes_no_group() {
    let rig = rig_after(Duration::ZERO);
    for n in 0..300 {
        let group = format!("made-up{n}");
        let r = rig
            .join(4, join_request(&group, "invented", None, &[("range", "m")]))
            .await;
        assert_eq!(r.error_code, UNKNOWN_MEMBER_ID);
        // A static member with an id of its own, in a group that has never been there.
        let r = rig
            .join(
                5,
                join_request(&group, "invented", Some("pod"), &[("range", "m")]),
            )
            .await;
        assert_eq!(r.error_code, UNKNOWN_MEMBER_ID);
    }
    assert!(
        rig.list_groups(&[], &[]).await.is_empty(),
        "nothing was made, so nothing is left for ever"
    );
}
