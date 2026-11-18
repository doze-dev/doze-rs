//! The heartbeat's answer: the member's epoch, timings, tasks, endpoints and statuses, in Kafka's order.

use kafka_wire::ResponseError as E;

use super::member::Member;
use super::topology::MISSING_INTERNAL_TOPICS;
use super::{StreamsAnswer, StreamsGroup, StreamsHeartbeat, endpoints, status};
use crate::groups::consumer::Topics;

impl StreamsGroup {
    pub(super) fn answer(
        &self,
        hb: &StreamsHeartbeat,
        delayed: Option<&'static str>,
        moved: bool,
        new_group: bool,
        topics: &Topics,
    ) -> StreamsAnswer {
        let Some(m) = self.members.get(&hb.member_id) else {
            return StreamsAnswer::refused(E::UnknownServerError, "the member is gone");
        };
        let endpoints = (self.endpoint_epoch != hb.endpoint_epoch).then(|| {
            self.topology.as_ref().map_or_else(Vec::new, |t| {
                endpoints::build(self.members.values(), m, t, topics)
            })
        });
        let create = match self.configured.as_ref().and_then(|c| c.not_ready.as_ref()) {
            Some(n) if n.code == MISSING_INTERNAL_TOPICS => self
                .configured
                .as_ref()
                .map(|c| c.create.clone())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        StreamsAnswer {
            error: None,
            member_id: Some(m.id.clone()),
            epoch: m.held.epoch,
            heartbeat_ms: hb.timing.heartbeat_ms,
            task_offset_ms: hb.timing.task_offset_ms,
            acceptable_lag: hb.timing.acceptable_lag,
            status: self.statuses(hb, m, delayed),
            tasks: (hb.epoch == 0 || moved).then(|| m.held.tasks.clone()),
            endpoint_epoch: if new_group { 0 } else { self.endpoint_epoch },
            endpoints,
            create,
        }
    }

    /// The statuses, in Kafka's order.
    fn statuses(
        &self,
        hb: &StreamsHeartbeat,
        m: &Member,
        delayed: Option<&'static str>,
    ) -> Vec<(i8, String)> {
        let mut out = Vec::new();
        if let Some(t) = self
            .topology
            .as_ref()
            .filter(|t| m.topology_epoch < t.epoch)
        {
            out.push((
                status::STALE_TOPOLOGY,
                format!(
                    "The member's topology epoch {} is behind the group's topology epoch {}.",
                    m.topology_epoch, t.epoch
                ),
            ));
        }
        if let Some(words) = delayed {
            out.push((status::ASSIGNMENT_DELAYED, words.to_owned()));
        }
        if let Some(n) = self.configured.as_ref().and_then(|c| c.not_ready.as_ref()) {
            out.push((n.code, n.detail.clone()));
        }
        if let Some(by) = &self.shutdown_by {
            out.push((
                status::SHUTDOWN_APPLICATION,
                format!(
                    "Streams group member {by} encountered a fatal error and requested a shutdown for the entire application."
                ),
            ));
        }
        let missing: Vec<&str> = hb
            .timing
            .tags
            .iter()
            .map(String::as_str)
            .filter(|t| !m.client_tags.contains_key(*t))
            .collect();
        if hb.version >= 1 && !missing.is_empty() {
            out.push((
                status::MISSING_CLIENT_TAGS,
                format!(
                    "Missing required client tags for rack-aware standby assignment: [{}]. Configure them via 'client.tag.<tagKey>' in your Streams config.",
                    missing.join(", ")
                ),
            ));
        }
        out
    }
}
