//! StreamsGroupHeartbeat (KIP-1071): one request that joins, keeps on and leaves a streams group, answered with the
//! member's tasks. Kafka 4.4.0's order (`KafkaApis.handleStreamsGroupHeartbeat`, then `GroupCoordinatorService`'s checks):
//! READ on the group; for a join, the topology's topics (none of Kafka's own, every name valid, DESCRIBE on each); then the
//! request's own checks in their words; then the group; then the internal topics it is missing are made, if the caller may
//! make them. The errors the handler makes before the group carry a code only, as Kafka's `getErrorResponse` writes them.

mod create;
mod wire;

pub(crate) use create::Failures;

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::ids::GroupId;
use kafka_wire::messages::{StreamsGroupHeartbeatRequest, StreamsGroupHeartbeatResponse};

use crate::groups::{Answer, Req, StreamsAnswer};
use crate::handlers::Client;
use crate::{Broker, Caller, Outcome};

/// The topics Kafka keeps for itself, which a topology may not name.
const KAFKA_INTERNAL: [&str; 3] = ["__consumer_offsets", "__transaction_state", "__share_group_state"];

/// Kafka's `Topic.isValid`: 1 to 249 characters of `[a-zA-Z0-9._-]`, and not `.` or `..`.
pub(crate) fn valid_topic_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 249
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

impl<F: Fs> Broker<F> {
    pub(crate) async fn streams_group_heartbeat(
        &self,
        version: i16,
        caller: &Caller,
        client: &Client<'_>,
        req: &StreamsGroupHeartbeatRequest,
    ) -> Outcome {
        let guard = self.guard(caller);
        if !guard.group(Operation::Read, req.group_id.0.as_str()) {
            return wire::respond_raw(StreamsGroupHeartbeatResponse::default(), E::GroupAuthorizationFailed);
        }
        if let Some(topology) = &req.topology {
            let names = wire::topology_names(topology);
            let internal: Vec<&str> = names.iter().copied().filter(|n| KAFKA_INTERNAL.contains(n)).collect();
            if !internal.is_empty() {
                return wire::respond(&StreamsAnswer::refused(
                    E::StreamsInvalidTopology,
                    format!(
                        "Use of Kafka internal topics {} in a Kafka Streams topology is prohibited.",
                        internal.join(",")
                    ),
                ));
            }
            let invalid: Vec<&str> = names.iter().copied().filter(|n| !valid_topic_name(n)).collect();
            if !invalid.is_empty() {
                return wire::respond(&StreamsAnswer::refused(
                    E::StreamsInvalidTopology,
                    format!("Topic names {} are not valid topic names.", invalid.join(",")),
                ));
            }
            if names.iter().any(|n| !guard.topic(Operation::Describe, n)) {
                return wire::respond_raw(StreamsGroupHeartbeatResponse::default(), E::TopicAuthorizationFailed);
            }
        }
        if let Err((error, words)) = wire::check(req) {
            return wire::respond(&StreamsAnswer::refused(error, words));
        }
        let Ok(group) = GroupId::parse(req.group_id.0.as_str()) else {
            return wire::respond(&StreamsAnswer::refused(E::InvalidRequest, "GroupId can't be empty."));
        };
        let Some(typed) = wire::typed(version, client, req, self.streams_timing(&group)) else {
            return wire::respond(&StreamsAnswer::refused(E::InvalidRequest, "MemberId can't be empty."));
        };
        let mut answer = match self.groups.ask(&group, Req::StreamsHeartbeat(Box::new(typed))).await {
            Some(Answer::StreamsHeartbeat(a)) => *a,
            _ => StreamsAnswer::refused(E::GroupIdNotFound, format!("Group {group} not found.")),
        };
        if !answer.create.is_empty() {
            self.make_internal_topics(caller, &mut answer).await;
        }
        wire::respond(&answer)
    }

    /// What a streams group runs by: its own configs where set, else the broker's.
    fn streams_timing(&self, group: &GroupId) -> crate::groups::StreamsTiming {
        let s = self.group_configs.streams(group);
        let millis = |ms: i64| u64::try_from(ms).unwrap_or(0);
        crate::groups::StreamsTiming {
            session_ms: millis(s.session_ms),
            heartbeat_ms: i32::try_from(s.heartbeat_ms).unwrap_or(i32::MAX),
            initial_delay_ms: millis(s.initial_delay_ms),
            assign_interval_ms: millis(s.assign_interval_ms),
            task_offset_ms: i32::try_from(s.task_offset_ms).unwrap_or(i32::MAX),
            acceptable_lag: s.acceptable_lag,
            standbys: usize::try_from(s.standbys).unwrap_or(0),
            tags: s.tags,
            max_size: crate::group_configs::streams::defaults::MAX_SIZE,
        }
    }
}
