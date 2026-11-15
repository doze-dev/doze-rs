//! ShareGroupHeartbeat (KIP-932): one request that joins, keeps on and leaves a share group, answered with what the member
//! is to read. Kafka 4.4.0's order (`KafkaApis.handleShareGroupHeartbeat`, then `GroupCoordinatorService`'s checks): share
//! groups on, READ on the group, a member id, DESCRIBE on each subscribed topic, then the request's own checks in their
//! words. The errors the handler makes carry a code only, as Kafka's `getErrorResponse` writes them; the group's carry its
//! words.

use std::time::Duration;

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::StrBytes;
use kafka_wire::ids::{GroupId, MemberId};
use kafka_wire::messages::share_group_heartbeat_response::{Assignment, TopicPartitions};
use kafka_wire::messages::{ShareGroupHeartbeatRequest, ShareGroupHeartbeatResponse};

use super::consumer_group_heartbeat::by_topic;
use crate::group_configs::defaults::MAX_SIZE;
use crate::groups::{Answer, Req, ShareAnswer, ShareHeartbeat, ShareTiming};
use crate::handlers::Client;
use crate::{Broker, Caller, Outcome};

/// Kafka's `isMemberIdValid`: present, not empty, and no longer than a UUID's text.
pub(crate) fn member_id_valid(id: Option<&str>) -> bool {
    id.is_some_and(|id| !id.is_empty() && id.chars().count() <= 36)
}

/// The longest rack id or topic name a member is kept with.
const MOST_BYTES: usize = 4096;

impl<F: Fs> Broker<F> {
    pub(crate) async fn share_group_heartbeat(
        &self,
        caller: &Caller,
        client: &Client<'_>,
        req: &ShareGroupHeartbeatRequest,
    ) -> Outcome {
        let guard = self.guard(caller);
        let refused = if !guard.group(Operation::Read, req.group_id.0.as_str()) {
            Some(E::GroupAuthorizationFailed)
        } else if !member_id_valid(Some(req.member_id.as_str())) {
            Some(E::InvalidRequest)
        } else if req
            .subscribed_topic_names
            .iter()
            .flatten()
            .any(|t| !guard.topic(Operation::Describe, t.0.as_str()))
        {
            Some(E::TopicAuthorizationFailed)
        } else {
            None
        };
        if let Some(error) = refused {
            let mut resp = ShareGroupHeartbeatResponse::default();
            resp.error_code = error.code();
            return respond_raw(resp);
        }
        let typed = match self.check_share_heartbeat(client, req) {
            Ok(typed) => typed,
            Err(words) => return respond(&ShareAnswer::refused(E::InvalidRequest, words)),
        };
        let Ok(group) = GroupId::parse(req.group_id.0.as_str()) else {
            return respond(&ShareAnswer::refused(
                E::InvalidRequest,
                "the group id is not usable",
            ));
        };
        let answer = match self.groups.ask(&group, Req::ShareHeartbeat(typed)).await {
            Some(Answer::ShareHeartbeat(a)) => a,
            _ => ShareAnswer::refused(E::GroupIdNotFound, format!("Group {group} not found.")),
        };
        // The share-partitions of what the group reads are made before the member is told to read them.
        if let Some((epoch, topics)) = &answer.initialize {
            self.initialize_shares(&group, *epoch, topics);
        }
        respond(&answer)
    }

    /// Kafka's checks of the request alone (`throwIfShareGroupHeartbeatRequestIsInvalid`), and the typed request.
    fn check_share_heartbeat(
        &self,
        client: &Client<'_>,
        r: &ShareGroupHeartbeatRequest,
    ) -> Result<ShareHeartbeat, &'static str> {
        if r.member_id.as_str().trim().is_empty() {
            return Err("MemberId can't be empty.");
        }
        if r.group_id.0.as_str().trim().is_empty() {
            return Err("GroupId can't be empty.");
        }
        if r.rack_id
            .as_ref()
            .is_some_and(|rack| rack.as_str().trim().is_empty())
        {
            return Err("RackId can't be empty.");
        }
        if r.member_epoch == 0 && r.subscribed_topic_names.as_ref().is_none_or(Vec::is_empty) {
            return Err("SubscribedTopicNames must be set in first request.");
        }
        if r.member_epoch < -1 {
            return Err("MemberEpoch is invalid.");
        }
        let too_long = |s: &str| s.len() > MOST_BYTES;
        let rack = r.rack_id.as_ref().map(|s| s.as_str().to_owned());
        let names: Option<Vec<String>> = r
            .subscribed_topic_names
            .as_ref()
            .map(|n| n.iter().map(|t| t.0.as_str().to_owned()).collect());
        if rack.as_deref().is_some_and(too_long) || names.iter().flatten().any(|n| too_long(n)) {
            return Err("a name is too long to keep");
        }
        let member_id = MemberId::new(r.member_id.as_str()).ok_or("MemberId can't be empty.")?;
        let group =
            GroupId::parse(r.group_id.0.as_str()).map_err(|_| "the group id is not usable")?;
        let s = self.share_settings(&group);
        let millis = |ms: i64| u64::try_from(ms).unwrap_or(0);
        let assign_interval_ms = self
            .config
            .share_assignment_interval
            .filter(|_| {
                !self
                    .group_configs
                    .of(&group)
                    .contains_key(crate::group_configs::ASSIGN)
            })
            .map_or(millis(s.assign_interval_ms), |d: Duration| {
                u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
            });
        Ok(ShareHeartbeat {
            group: r.group_id.0.as_str().to_owned(),
            member_id,
            epoch: r.member_epoch,
            rack,
            subscribed: names,
            client_id: client.id.to_owned(),
            client_host: client.host.to_owned(),
            timing: ShareTiming {
                session_ms: millis(s.session_ms),
                heartbeat_ms: i32::try_from(s.heartbeat_ms).unwrap_or(i32::MAX),
                assign_interval_ms,
                max_size: MAX_SIZE,
            },
        })
    }
}

fn respond_raw(resp: ShareGroupHeartbeatResponse) -> Outcome {
    Outcome::Respond(kafka_wire::api::Response::ShareGroupHeartbeat(resp))
}

fn respond(a: &ShareAnswer) -> Outcome {
    let mut resp = ShareGroupHeartbeatResponse::default();
    if let Some((error, words)) = &a.error {
        resp.error_code = error.code();
        resp.error_message = Some(StrBytes::from_string(words.clone()));
        return respond_raw(resp);
    }
    resp.member_id = a
        .member_id
        .as_ref()
        .map(|m| StrBytes::from_string(m.to_string()));
    resp.member_epoch = a.epoch;
    resp.heartbeat_interval_ms = a.heartbeat_ms;
    resp.assignment = a.assignment.as_ref().map(|parts| {
        let mut out = Assignment::default();
        out.topic_partitions = by_topic(parts)
            .into_iter()
            .map(|(topic_id, partitions)| {
                let mut tp = TopicPartitions::default();
                tp.topic_id = topic_id;
                tp.partitions = partitions;
                tp
            })
            .collect();
        out
    });
    respond_raw(resp)
}
