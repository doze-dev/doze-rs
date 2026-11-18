//! The internal topics a streams group is missing, made for it (Kafka 4.4.0's `KafkaApis.handleStreamsGroupHeartbeat` and
//! `AutoTopicCreationManager.createStreamsInternalTopics`): CREATE on the cluster, or on each topic, is needed; if any is
//! refused none is made and the status says so. Kafka makes them without waiting and remembers a failure for twice the
//! heartbeat interval, during which the topic is not tried again and later heartbeats are told of it. Here they are
//! made before the answer goes (the answer is the same: it was worked out before), and failures are remembered the same
//! way. The memory holds nothing unless a creation failed, and is cleared as it is read.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::StrBytes;
use kafka_wire::messages::CreateTopicsRequest;
use kafka_wire::messages::create_topics_request::{CreatableTopic, CreatableTopicConfig};

use crate::groups::StreamsAnswer;
use crate::groups::streams::topology::{Creatable, MISSING_INTERNAL_TOPICS, java_hash_order};
use crate::quotas::Mutations;
use crate::{Broker, Caller};

/// Internal topics whose creation failed lately: the error's words, and until when it is remembered.
#[derive(Debug, Default)]
pub(crate) struct Failures(std::sync::Mutex<HashMap<String, (String, Instant)>>);

impl Failures {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, (String, Instant)>> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn request_of(topics: &[&Creatable]) -> CreateTopicsRequest {
    let mut req = CreateTopicsRequest::default();
    req.timeout_ms = 30_000;
    req.topics = topics
        .iter()
        .map(|c| {
            let mut t = CreatableTopic::default();
            t.name = kafka_wire::messages::TopicName(StrBytes::from_string(c.name.clone()));
            t.num_partitions = c.partitions;
            t.replication_factor = c.replication;
            t.configs = c
                .configs
                .iter()
                .map(|(k, v)| {
                    let mut kv = CreatableTopicConfig::default();
                    kv.name = StrBytes::from_string(k.clone());
                    kv.value = Some(StrBytes::from_string(v.clone()));
                    kv
                })
                .collect();
            t
        })
        .collect();
    req
}

/// Adds `more` to the MISSING_INTERNAL_TOPICS status's words, or adds that status with them.
fn tell(answer: &mut StreamsAnswer, more: &str) {
    match answer.status.iter_mut().find(|(code, _)| *code == MISSING_INTERNAL_TOPICS) {
        Some((_, detail)) => {
            detail.push_str("; ");
            detail.push_str(more);
        }
        None => answer.status.push((MISSING_INTERNAL_TOPICS, more.to_owned())),
    }
}

impl<F: Fs> Broker<F> {
    pub(super) async fn make_internal_topics(&self, caller: &Caller, answer: &mut StreamsAnswer) {
        let creatable = std::mem::take(&mut answer.create);
        let guard = self.guard(caller);
        if !guard.cluster(Operation::Create) {
            // Named in the order Kafka's map of the topics to make has them.
            let names = creatable.iter().map(|c| c.name.as_str());
            let refused: Vec<&str> = java_hash_order(names)
                .into_iter()
                .filter(|n| !guard.topic(Operation::Create, n))
                .collect();
            if !refused.is_empty() {
                tell(answer, &format!("Unauthorized to CREATE on topics {}.", refused.join(", ")));
                return;
            }
        }
        let now = Instant::now();
        let failed: Vec<(String, String)> = {
            let mut known = self.streams_topic_failures.lock();
            known.retain(|_, (_, until)| *until > now);
            creatable
                .iter()
                .filter_map(|c| known.get(&c.name).map(|(why, _)| (c.name.clone(), why.clone())))
                .collect()
        };
        let to_make: Vec<&Creatable> = creatable
            .iter()
            .filter(|c| !failed.iter().any(|(n, _)| *n == c.name))
            .collect();
        if !to_make.is_empty() {
            let resp = self
                .create_topics(caller, &request_of(&to_make), &mut Mutations::unbounded())
                .await;
            let remember = Duration::from_millis(u64::try_from(answer.heartbeat_ms).unwrap_or(0) * 2);
            let mut known = self.streams_topic_failures.lock();
            for t in resp.topics.iter().filter(|t| t.error_code != 0) {
                let why = t.error_message.as_ref().map(ToString::to_string).filter(|m| !m.is_empty()).unwrap_or_else(|| {
                    kafka_wire::ResponseError::try_from_code(t.error_code).map_or_else(String::new, |e| e.to_string())
                });
                known.insert(t.name.0.to_string(), (why, now + remember));
            }
        }
        if !failed.is_empty() {
            let shown: Vec<String> = failed.iter().take(3).map(|(n, why)| format!("{n} ({why})")).collect();
            let more = match failed.len() {
                n if n > 3 => format!(" and {} more", n - 3),
                _ => String::new(),
            };
            tell(answer, &format!("Creation failed: {}{more}.", shown.join(", ")));
        }
    }
}
