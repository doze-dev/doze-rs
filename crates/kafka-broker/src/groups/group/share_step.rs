//! A share group's step: its own heartbeat, and Kafka's refusals of every other kind's requests.

use super::super::consumer::{self, TopicSource};
use super::super::machine::{Answer, Effect, Event, Millis, Token};
use super::super::share::ShareGroup;
use super::super::streams::StreamsAnswer;
use kafka_wire::ResponseError as E;

/// A share group's step: its own heartbeat, and Kafka's refusals of every other kind's requests
/// (`GroupMetadataManager`'s `getOrMaybeCreate*Group`: "Group %s is not a ... group.").
pub(super) fn step_share(
    s: &mut ShareGroup,
    event: Event,
    now: Millis,
    source: &dyn TopicSource,
) -> Vec<Effect> {
    let mut out = Vec::new();
    match event {
        Event::ShareHeartbeat(token, hb) => {
            s.heartbeat(token, &hb, &source.topics(), now, &mut out)
        }
        Event::Timer(key) => s.timer(&key, now, &mut out),
        Event::TopicsChanged => s.topics_changed(),
        Event::Describe(token) => out.push(describe_share(s, token, now, source)),
        Event::Delete(token) => {
            let refused = (s.member_count() > 0).then_some(E::NonEmptyGroup);
            out.push(Effect::Reply(token, Answer::Delete(refused)));
        }
        Event::Force(token, force) => {
            let done = force_share(s, force, now, &mut out);
            out.push(Effect::Reply(token, Answer::Forced(done)));
        }
        other => out.extend(refuse_in_share(other)),
    }
    out
}

/// A share group as DescribeGroups and the console see it.
fn describe_share(s: &ShareGroup, token: Token, now: Millis, source: &dyn TopicSource) -> Effect {
    use super::super::machine::{Description, State};
    #[cfg(not(feature = "inspect"))]
    let _ = now;
    let view = s.view(&source.topics());
    Effect::Reply(
        token,
        Answer::Describe(Description {
            state: if view.members.is_empty() {
                State::Empty
            } else {
                State::Stable
            },
            protocol_type: Some("share".to_owned()),
            protocol: None,
            members: Vec::new(),
            subscribed: None,
            consumer: None,
            share: Some(view),
            streams: None,
            #[cfg(feature = "inspect")]
            inspect: Box::new(s.inspect(now)),
        }),
    )
}

/// The Lab's forcing of a share group: a member's session run out, or the group epoch moved.
fn force_share(
    s: &mut ShareGroup,
    force: super::super::machine::Force,
    now: Millis,
    out: &mut Vec<Effect>,
) -> super::super::machine::Forced {
    use super::super::machine::{Force, Forced};
    match force {
        Force::Expire(id) if s.force_expire(&id, now, out) => Forced::Done,
        Force::Expire(_) => Forced::NoSuchMember,
        Force::Rebalance if s.force_bump(now, out) => Forced::Done,
        Force::Rebalance => Forced::Empty,
    }
}

/// What a share group answers the other kinds' requests: a classic JoinGroup is told its protocol is not the group's;
/// the rest that the group is not theirs (a share group has no committed offsets: "Group %s is not a consumer group.").
fn refuse_in_share(event: Event) -> Option<Effect> {
    use super::super::machine::{JoinAnswer, LeaveResult, SyncAnswer};
    let (token, answer) = match event {
        Event::Join(token, _) => (
            token,
            Answer::Join(JoinAnswer::refused(E::InconsistentGroupProtocol, None)),
        ),
        Event::Sync(token, _) => (token, Answer::Sync(SyncAnswer::refused(E::GroupIdNotFound))),
        Event::Heartbeat(token, _) => (token, Answer::Heartbeat(Some(E::GroupIdNotFound))),
        Event::Leave(token, targets) => (
            token,
            Answer::Leave(
                targets
                    .into_iter()
                    .map(|t| LeaveResult {
                        member_id: t.member_id,
                        instance: t.instance,
                        error: Some(E::GroupIdNotFound),
                    })
                    .collect(),
            ),
        ),
        Event::Commit(token, _) => (token, Answer::Commit(Some(E::GroupIdNotFound))),
        Event::ConsumerHeartbeat(token, hb) => (
            token,
            Answer::ConsumerHeartbeat(consumer::HeartbeatAnswer::refused(
                E::GroupIdNotFound,
                format!("Group {} is not a consumer group.", hb.group),
            )),
        ),
        Event::StreamsHeartbeat(token, hb) => (
            token,
            Answer::StreamsHeartbeat(StreamsAnswer::refused(
                E::GroupIdNotFound,
                format!("Group {} is not a streams group.", hb.group),
            )),
        ),
        // A share group's own: answered by `step_share`.
        Event::ShareHeartbeat(..)
        | Event::Timer(_)
        | Event::TopicsChanged
        | Event::Describe(_)
        | Event::Delete(_)
        | Event::Force(..) => return None,
    };
    Some(Effect::Reply(token, answer))
}

