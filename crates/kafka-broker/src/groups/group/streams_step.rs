//! A streams group's step: its own heartbeat, the offset commits it fences, and Kafka's refusals of the other kinds'
//! requests (`GroupMetadataManager`'s `getOrMaybeCreate*Group`: "Group %s is not a ... group.").

use super::super::consumer::{self, TopicSource};
use super::super::machine::{
    Answer, Description, Effect, Event, Force, Forced, JoinAnswer, LeaveResult, Millis, State, SyncAnswer, Token,
};
use super::super::share::ShareAnswer;
use super::super::streams::StreamsGroup;
use kafka_wire::ResponseError as E;

pub(super) fn step_streams(
    s: &mut StreamsGroup,
    event: Event,
    now: Millis,
    source: &dyn TopicSource,
) -> Vec<Effect> {
    let mut out = Vec::new();
    match event {
        Event::StreamsHeartbeat(token, hb) => s.heartbeat(token, &hb, &source.topics(), now, &mut out),
        Event::Timer(key) => s.timer(&key, now, &mut out),
        Event::TopicsChanged => s.topics_changed(),
        Event::Commit(token, req) => {
            let fenced = s.commit_fence(&req, &source.topics());
            out.push(Effect::Reply(token, Answer::Commit(fenced)));
        }
        Event::Describe(token) => out.push(describe(s, token, now)),
        Event::Delete(token) => {
            let refused = (s.member_count() > 0).then_some(E::NonEmptyGroup);
            out.push(Effect::Reply(token, Answer::Delete(refused)));
        }
        Event::Force(token, force) => {
            let done = match force {
                Force::Expire(id) if s.force_expire(&id, now, &mut out) => Forced::Done,
                Force::Expire(_) => Forced::NoSuchMember,
                Force::Rebalance if s.force_bump(now, &mut out) => Forced::Done,
                Force::Rebalance => Forced::Empty,
            };
            out.push(Effect::Reply(token, Answer::Forced(done)));
        }
        other => out.extend(refuse(other)),
    }
    out
}

/// A streams group as DescribeGroups, ListGroups and the console see it.
fn describe(s: &StreamsGroup, token: Token, now: Millis) -> Effect {
    #[cfg(not(feature = "inspect"))]
    let _ = now;
    Effect::Reply(
        token,
        Answer::Describe(Description {
            state: if s.member_count() == 0 {
                State::Empty
            } else {
                State::Stable
            },
            protocol_type: Some("streams".to_owned()),
            protocol: None,
            members: Vec::new(),
            subscribed: Some(s.subscribed()),
            consumer: None,
            share: None,
            streams: Some(Box::new(s.view())),
            #[cfg(feature = "inspect")]
            inspect: Box::new(s.inspect(now)),
        }),
    )
}

/// What a streams group answers the other kinds' requests.
fn refuse(event: Event) -> Option<Effect> {
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
        Event::ConsumerHeartbeat(token, hb) => (
            token,
            Answer::ConsumerHeartbeat(consumer::HeartbeatAnswer::refused(
                E::GroupIdNotFound,
                format!("Group {} is not a consumer group.", hb.group),
            )),
        ),
        Event::ShareHeartbeat(token, hb) => (
            token,
            Answer::ShareHeartbeat(ShareAnswer::refused(
                E::GroupIdNotFound,
                format!("Group {} is not a share group.", hb.group),
            )),
        ),
        // A streams group's own: answered by `step_streams`.
        Event::StreamsHeartbeat(..)
        | Event::Commit(..)
        | Event::Timer(_)
        | Event::TopicsChanged
        | Event::Describe(_)
        | Event::Delete(_)
        | Event::Force(..) => return None,
    };
    Some(Effect::Reply(token, answer))
}
