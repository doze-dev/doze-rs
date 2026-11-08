//! Committed offsets expire, by the rules a real Kafka applies. (The rules themselves are tested in `retention.rs` of
//! the library, with times given; these check that the broker applies them to what it has and records.)
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use kafka_storage::sim::{Failure, Op};
use support::{join_request, open_on_with, rig_with, subscription, topic};

const RETENTION: Duration = Duration::from_millis(80);
const COORDINATOR_NOT_AVAILABLE: i16 = 15;

fn config(retention: Duration) -> impl FnOnce(&mut kafka_broker::Config) {
    move |c| {
        c.group_initial_rebalance_delay = Duration::ZERO;
        c.offsets_retention = retention;
    }
}

async fn rig(retention: Duration) -> support::Rig {
    let rig = rig_with(config(retention));
    rig.create(vec![topic("t", 1), topic("u", 1)], false).await;
    rig
}

/// Longer than the retention, as the clock the broker keeps runs.
async fn past_the_retention() {
    tokio::time::sleep(RETENTION * 2).await;
}

async fn committed(rig: &support::Rig, group: &str) -> Vec<String> {
    rig.fetch_offsets(false, group, None)
        .await
        .into_iter()
        .map(|(t, ..)| t)
        .collect()
}

/// A member of `group` subscribed to `topics`, stable.
async fn member(rig: &support::Rig, group: &str, topics: &[&str]) -> String {
    let meta = subscription(topics);
    let joined = rig
        .join(3, join_request(group, "", None, &[("range", &meta)]))
        .await;
    let id = joined.member_id.to_string();
    rig.sync(3, group, &id, 1, &[(&id, "")]).await;
    id
}

#[tokio::test]
async fn a_group_that_never_had_a_member_loses_each_offset_after_the_retention_from_its_own_commit()
{
    let rig = rig(RETENTION).await;
    rig.commit_offsets(8, "solo", "", -1, &[("t", 0, 1, None)])
        .await;
    rig.broker.check_offsets().await;
    assert_eq!(committed(&rig, "solo").await, ["t"], "not yet");
    past_the_retention().await;
    rig.commit_offsets(8, "solo", "", -1, &[("u", 0, 2, None)])
        .await;
    rig.broker.check_offsets().await;
    assert_eq!(
        committed(&rig, "solo").await,
        ["u"],
        "the old one went, the new one stays"
    );
    past_the_retention().await;
    rig.broker.check_offsets().await;
    assert!(committed(&rig, "solo").await.is_empty());
    assert!(
        rig.list_groups(&[], &[]).await.is_empty(),
        "and the group with it"
    );
}

#[tokio::test]
async fn a_group_with_members_keeps_what_it_subscribes_to_and_loses_the_rest_by_age() {
    let rig = rig(RETENTION).await;
    let id = member(&rig, "g", &["t"]).await;
    rig.commit_offsets(8, "g", &id, 1, &[("t", 0, 1, None), ("u", 0, 2, None)])
        .await;
    past_the_retention().await;
    rig.broker.check_offsets().await;
    assert_eq!(committed(&rig, "g").await, ["t"]);
}

#[tokio::test]
async fn an_emptied_group_keeps_its_offsets_for_the_retention_from_the_moment_it_emptied() {
    let rig = rig(RETENTION).await;
    let id = member(&rig, "g", &["t"]).await;
    rig.commit_offsets(8, "g", &id, 1, &[("t", 0, 1, None)])
        .await;
    // The offset is older than the retention by the time the member leaves; the group is not.
    past_the_retention().await;
    rig.leave(3, "g", &[&id]).await;
    rig.broker.check_offsets().await;
    assert_eq!(committed(&rig, "g").await, ["t"], "emptied just now");
    // A commit into the empty group does not extend it.
    rig.commit_offsets(8, "g", "", -1, &[("u", 0, 9, None)])
        .await;
    past_the_retention().await;
    rig.broker.check_offsets().await;
    assert!(
        committed(&rig, "g").await.is_empty(),
        "all of it went with the group's retention"
    );
    assert_eq!(
        rig.describe_groups(&["g"]).await.groups[0]
            .group_state
            .as_str(),
        "Dead"
    );
    // And the id is a new group when used again.
    let again = member(&rig, "g", &["t"]).await;
    assert!(again.starts_with("doze-"));
}

#[tokio::test]
async fn a_restart_changes_none_of_this_and_the_emptied_group_is_listed_as_what_it_was() {
    let rig = rig(RETENTION).await;
    let id = member(&rig, "g", &["t"]).await;
    rig.commit_offsets(8, "g", &id, 1, &[("t", 0, 1, None)])
        .await;
    rig.leave(3, "g", &[&id]).await;
    let after = open_on_with(rig.fs.process_restart(), config(RETENTION));
    assert_eq!(
        after.list_groups(&[], &[]).await,
        [("g".into(), "Empty".into(), "consumer".into())]
    );
    assert_eq!(
        after.describe_groups(&["g"]).await.groups[0]
            .protocol_type
            .as_str(),
        "consumer"
    );
    past_the_retention().await;
    after.broker.check_offsets().await;
    assert!(
        committed(&after, "g").await.is_empty(),
        "emptied before the restart, expired after"
    );
    assert_eq!(
        after.describe_groups(&["g"]).await.groups[0]
            .group_state
            .as_str(),
        "Dead"
    );
}

#[tokio::test]
async fn a_group_that_had_members_when_the_broker_stopped_is_taken_to_have_them_until_their_session_ends()
 {
    let rig = rig(RETENTION).await;
    let id = member(&rig, "g", &["t"]).await;
    rig.commit_offsets(8, "g", &id, 1, &[("t", 0, 1, None)])
        .await;
    // Killed with the member there: the session was ten seconds.
    let after = open_on_with(rig.fs.process_restart(), config(RETENTION));
    past_the_retention().await;
    after.broker.check_offsets().await;
    assert_eq!(
        committed(&after, "g").await,
        ["t"],
        "the member may yet come back"
    );
}

#[tokio::test]
async fn a_retention_of_zero_keeps_offsets_for_ever_but_a_group_with_nothing_is_still_forgotten() {
    let rig = rig(Duration::ZERO).await;
    rig.commit_offsets(8, "solo", "", -1, &[("t", 0, 1, None)])
        .await;
    let id = member(&rig, "g", &["t"]).await;
    rig.leave(3, "g", &[&id]).await;
    past_the_retention().await;
    rig.broker.check_offsets().await;
    assert_eq!(committed(&rig, "solo").await, ["t"]);
    assert_eq!(
        rig.describe_groups(&["g"]).await.groups[0]
            .group_state
            .as_str(),
        "Dead"
    );
}

#[tokio::test]
async fn a_member_is_not_seated_when_the_record_of_its_group_cannot_be_written() {
    let rig = rig(RETENTION).await;
    rig.fs.fail(Op::Fsync, 1, Failure::Io);
    let joined = rig
        .join(
            3,
            join_request("g", "", None, &[("range", &subscription(&["t"]))]),
        )
        .await;
    assert_eq!(joined.error_code, COORDINATOR_NOT_AVAILABLE);
    assert!(
        rig.describe_groups(&["g"]).await.groups[0]
            .members
            .is_empty()
    );
}

#[tokio::test]
async fn a_group_that_has_only_issued_an_id_is_not_forgotten_before_the_client_comes_back_with_it()
{
    let rig = rig(RETENTION).await;
    let meta = subscription(&["t"]);
    let first = rig
        .join(4, join_request("g", "", None, &[("range", &meta)]))
        .await;
    assert_eq!(first.error_code, 79, "told to come back with the id");
    past_the_retention().await;
    rig.broker.check_offsets().await;
    let id = first.member_id.to_string();
    let joined = rig
        .join(4, join_request("g", &id, None, &[("range", &meta)]))
        .await;
    assert_eq!(joined.error_code, 0, "the id is still good after a sweep");
}

/// A group whose member has left and committed; returns nothing but leaves it empty with an offset.
async fn emptied(rig: &support::Rig, group: &str) {
    let id = member(rig, group, &["t"]).await;
    rig.commit_offsets(8, group, &id, 1, &[("t", 0, 1, None)])
        .await;
    rig.leave(3, group, &[&id]).await;
}

#[tokio::test]
async fn a_join_that_is_refused_does_not_keep_an_emptied_groups_offsets_for_ever() {
    let rig = rig(RETENTION).await;
    emptied(&rig, "g").await;
    let meta = subscription(&["t"]);
    // Refused for its session; and told to come back with an id it never does.
    let mut quick = join_request("g", "", None, &[("range", &meta)]);
    quick.session_timeout_ms = 5;
    assert_eq!(rig.join(3, quick).await.error_code, 26);
    assert_eq!(
        rig.join(4, join_request("g", "", None, &[("range", &meta)]))
            .await
            .error_code,
        79
    );
    past_the_retention().await;
    rig.broker.check_offsets().await;
    assert!(
        committed(&rig, "g").await.is_empty(),
        "the group stayed empty, so its offsets went"
    );
}

#[tokio::test]
async fn a_group_recorded_as_having_members_is_not_forgotten_by_the_first_sweep_after_a_restart() {
    let rig = rig(RETENTION).await;
    member(&rig, "g", &["t"]).await; // no offsets
    let after = open_on_with(rig.fs.process_restart(), config(RETENTION));
    past_the_retention().await;
    after.broker.check_offsets().await;
    // Its member may still come back: it is listed, as the empty group it is taken to be, until their session has run out.
    assert_eq!(
        after.list_groups(&[], &[]).await,
        [("g".into(), "Empty".into(), "consumer".into())]
    );
}
