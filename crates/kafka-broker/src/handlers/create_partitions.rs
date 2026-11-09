//! CreatePartitions: gives a topic more partitions (source: `ControllerApis.createPartitions`,
//! `ReplicationControlManager.createPartitions`, `validateManualPartitionAssignment`).

use std::collections::HashMap;

use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_storage::topic_dirs;
use kafka_wire::ResponseError as E;
use kafka_wire::StrBytes;
use kafka_wire::api::Response;
use kafka_wire::ids::{PartitionCount, TopicName};
use kafka_wire::messages::create_partitions_request::CreatePartitionsTopic;
use kafka_wire::messages::create_partitions_response::CreatePartitionsTopicResult;
use kafka_wire::messages::{CreatePartitionsRequest, CreatePartitionsResponse};
use tracing::warn;

use crate::{Broker, Outcome, blocking};

/// Why a topic was not given partitions: the code and Kafka's message (none, for an unknown topic).
type Refusal = (E, Option<String>);

fn refuse(code: E, message: impl Into<String>) -> Refusal {
    (code, Some(message.into()))
}

impl<F: Fs> Broker<F> {
    /// Each topic named is answered once: a name listed twice is refused (both entries), first in the answer; the rest
    /// follow in the order asked, a refusal of one not stopping the others.
    pub(crate) async fn create_partitions(&self, req: &CreatePartitionsRequest) -> Outcome {
        let _one_change_at_a_time = self.topic_admin.lock().await;
        let mut count: HashMap<&str, usize> = HashMap::new();
        for t in &req.topics {
            *count.entry(t.name.0.as_str()).or_default() += 1;
        }
        let mut resp = CreatePartitionsResponse::default();
        let mut answered = std::collections::HashSet::new();
        for t in &req.topics {
            let raw = t.name.0.as_str();
            if count.get(raw).copied().unwrap_or(0) > 1 && answered.insert(raw) {
                let dup = refuse(E::InvalidRequest, "Duplicate topic name.");
                resp.results.push(answer(t, Err(dup)));
            }
        }
        let mut grown = false;
        for t in &req.topics {
            if count.get(t.name.0.as_str()).copied().unwrap_or(0) > 1 {
                continue;
            }
            let done = blocking::run(|| self.grow(t, req.validate_only));
            grown |= matches!(done, Ok(true));
            resp.results.push(answer(t, done.map(|_| ())));
        }
        if grown {
            // A consumer group that subscribes to the topic is waiting to be given the new partitions.
            self.groups.topics_changed().await;
        }
        Outcome::Respond(Response::CreatePartitions(resp))
    }

    /// Gives one topic its new partitions. `Ok(true)` if it now has more, `Ok(false)` if it only could.
    fn grow(&self, t: &CreatePartitionsTopic, validate_only: bool) -> Result<bool, Refusal> {
        let topic = TopicName::parse(&t.name.0)
            .ok()
            .and_then(|n| self.meta.topic(&n))
            .ok_or((E::UnknownTopicOrPartition, None))?;
        let have = topic.partitions.get();
        if t.count == have {
            return Err(refuse(
                E::InvalidPartitions,
                format!("Topic already has {have} partition(s)."),
            ));
        }
        if t.count < have {
            return Err(refuse(
                E::InvalidPartitions,
                format!(
                    "The topic {} currently has {have} partition(s); {} would not be an increase.",
                    t.name.0, t.count
                ),
            ));
        }
        let additional = t.count - have;
        if let Some(assignments) = &t.assignments
            && usize::try_from(additional).ok() != Some(assignments.len())
        {
            return Err(refuse(
                E::InvalidReplicaAssignment,
                format!(
                    "Attempted to add {additional} additional partition(s), but only {} assignment(s) were specified.",
                    assignments.len()
                ),
            ));
        }
        let limit = self.config.max_partitions;
        if t.count > limit {
            return Err(refuse(
                E::InvalidPartitions,
                format!(
                    "{} partitions is above this broker's limit of {limit} (--max-partitions)",
                    t.count
                ),
            ));
        }
        for a in t.assignments.iter().flatten() {
            self.check_assignment(&a.broker_ids.iter().map(|b| b.0).collect::<Vec<_>>())?;
        }
        if validate_only {
            return Ok(false);
        }
        let count = PartitionCount::new(t.count).ok_or((E::InvalidPartitions, None))?;
        self.add_partitions(&topic, count)?;
        Ok(true)
    }

    /// A manual assignment of one new partition: Kafka's checks, on a cluster of this one broker, whose partitions each
    /// have one replica.
    fn check_assignment(&self, brokers: &[i32]) -> Result<(), Refusal> {
        let bad = |m: String| refuse(E::InvalidReplicaAssignment, m);
        if brokers.is_empty() {
            return Err(bad(
                "The manual partition assignment includes an empty replica list.".to_owned(),
            ));
        }
        let mut sorted = brokers.to_vec();
        sorted.sort_unstable();
        let mut previous = None;
        for b in &sorted {
            if *b != self.config.node_id {
                return Err(bad(format!(
                    "The manual partition assignment includes broker {b}, but no such broker is registered."
                )));
            }
            if previous == Some(*b) {
                return Err(bad(format!(
                    "The manual partition assignment includes the broker {b} more than once."
                )));
            }
            previous = Some(*b);
        }
        if sorted.len() != 1 {
            return Err(bad(format!(
                "The manual partition assignment includes a partition with {} replica(s), but this is not consistent with previous partitions, which have 1 replica(s).",
                sorted.len()
            )));
        }
        Ok(())
    }

    /// Makes the new partitions' directories (and flushes them), then records the new count. The directories come first:
    /// a stop between the two leaves empty directories no topic has yet, which the next attempt reuses; the other
    /// way round it would leave a topic with partitions that have no place on disk.
    fn add_partitions(&self, topic: &Topic, count: PartitionCount) -> Result<(), Refusal> {
        let data = &self.config.data_dir;
        let from = usize::try_from(topic.partitions.get()).unwrap_or(0);
        let storage = |e: &dyn std::fmt::Display| {
            warn!(topic = %topic.name, error = %e, "could not add partitions");
            refuse(E::KafkaStorageError, "The partitions could not be made.")
        };
        let undo = || {
            if let Err(e) =
                topic_dirs::remove_partition_dirs_from(&self.fs, data, &topic.name, from, count)
            {
                warn!(topic = %topic.name, error = %e, "taking back the new partition directories failed");
            }
        };
        if let Err(e) =
            topic_dirs::create_partition_dirs_from(&self.fs, data, &topic.name, from, count)
        {
            undo();
            return Err(storage(&e));
        }
        let updated = self.meta.update_topic(&topic.name, |t| Topic {
            partitions: count,
            ..t.clone()
        });
        match updated {
            Ok(Some(_)) => Ok(()),
            Ok(None) => {
                undo();
                Err((E::UnknownTopicOrPartition, None))
            }
            Err(e) => {
                undo();
                Err(storage(&e))
            }
        }
    }
}

fn answer(t: &CreatePartitionsTopic, result: Result<(), Refusal>) -> CreatePartitionsTopicResult {
    let mut r = CreatePartitionsTopicResult::default();
    r.name = t.name.clone();
    if let Err((code, message)) = result {
        r.error_code = code.code();
        r.error_message = message.map(StrBytes::from_string);
    }
    r
}

#[cfg(test)]
mod tests;
