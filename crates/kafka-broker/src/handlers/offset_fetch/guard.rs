//! What an OffsetFetch may show of a group's offsets to a caller: only the topics it may DESCRIBE.
//!
//! Asked for topics by name, a caller is told 29 and -1 for each it may not (so a topic's existence is not
//! revealed); asked for every offset the group has, it is shown the rest and never told of the others
//! (`KafkaApis.scala:1090-1123`, `1124-1185`).

use std::collections::HashSet;

use kafka_auth::authz::operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::messages::{OffsetFetchResponse, TopicName as WireName};

use super::{Asked, Found, single};
use crate::Broker;
use crate::handlers::group_guard::Guard;

/// The topics asked for that the caller may not describe, with their partitions.
type Barred<'a> = Vec<(&'a WireName, &'a [i32])>;

impl<F: Fs> Broker<F> {
    /// [`Broker::answer`] for what the caller may see, and 29 with offset -1 for the topics it asked for and may not.
    pub(super) fn answer_guarded(
        &self,
        guard: &Guard<'_>,
        group: &str,
        asked: Asked<'_>,
        pending: &HashSet<(String, i32)>,
    ) -> Vec<(WireName, Vec<Found>)> {
        let every = asked.is_none();
        let (allowed, barred) = split(guard, asked);
        let mut out = self.answer(group, allowed, pending);
        if every {
            out.retain(|(name, _)| guard.topic(operation::DESCRIBE, name.as_str()));
        }
        out.extend(barred.into_iter().map(|(name, partitions)| {
            let refused = partitions
                .iter()
                .map(|p| Found {
                    error: ResponseError::TopicAuthorizationFailed.code(),
                    ..Found::none(*p)
                })
                .collect();
            (name.clone(), refused)
        }));
        out
    }
}

/// The topics asked for, divided into those the caller may describe and those it may not.
fn split<'a>(guard: &Guard<'_>, asked: Asked<'a>) -> (Asked<'a>, Barred<'a>) {
    let Some(topics) = asked else {
        return (None, Vec::new());
    };
    let (allowed, barred) = topics
        .into_iter()
        .partition(|(name, _)| guard.topic(operation::DESCRIBE, name.as_str()));
    (Some(allowed), barred)
}

/// The answer to a version before 8 when the caller may not describe the group: 30 at the top from v2, and before
/// that (v1 has no code there) 30 and -1 on each partition asked about (`OffsetFetchResponse.groupError`).
pub(super) fn refuse_old(version: i16, topics: &Asked<'_>, resp: &mut OffsetFetchResponse) {
    let error = ResponseError::GroupAuthorizationFailed.code();
    if version >= 2 {
        resp.error_code = error;
        return;
    }
    for (name, partitions) in topics.iter().flatten() {
        let mut topic =
            kafka_wire::messages::offset_fetch_response::OffsetFetchResponseTopic::default();
        topic.name = (*name).clone();
        topic.partitions = partitions
            .iter()
            .map(|p| {
                single(Found {
                    error,
                    ..Found::none(*p)
                })
            })
            .collect();
        resp.topics.push(topic);
    }
}
