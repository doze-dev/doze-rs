//! CreateTopics.

use std::collections::{BTreeMap, HashMap, HashSet};

use kafka_meta::{Collision, MetaError, Topic, random_topic_id};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::ids::{PartitionCount, TopicName, TopicNameError};
use kafka_wire::messages::create_topics_request::CreatableTopic;
use kafka_wire::messages::create_topics_response::{CreatableTopicConfigs, CreatableTopicResult};
use kafka_wire::messages::{CreateTopicsRequest, CreateTopicsResponse};

use crate::topic_config::{self, Entry};
use crate::{Broker, BrokerError};

type Refusal = (ResponseError, String);

impl<F: Fs> Broker<F> {
    /// Creates each topic of the request, and answers once per name. One refused topic does not stop the
    /// others.
    pub(crate) async fn create_topics(&self, req: &CreateTopicsRequest) -> CreateTopicsResponse {
        let _one_change_at_a_time = self.topic_admin.lock().await;
        crate::blocking::run(|| self.create_each(req))
    }

    fn create_each(&self, req: &CreateTopicsRequest) -> CreateTopicsResponse {
        // A name listed twice is a malformed request: neither entry is acted on, and Kafka answers once per name.
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for t in &req.topics {
            *counts.entry(&t.name).or_default() += 1;
        }
        let mut answered = HashSet::new();
        // The names this request has created so far. Kafka checks every name of a request against the topics
        // that were there before it, so two that differ only in '.' against '_' are both created when they
        // arrive together.
        let mut created = HashSet::new();
        let mut resp = CreateTopicsResponse::default();
        for t in &req.topics {
            let raw: &str = &t.name;
            if counts.get(raw).copied().unwrap_or(0) > 1 {
                if answered.insert(raw) {
                    let duplicate = (
                        ResponseError::InvalidRequest,
                        "Duplicate topic name.".to_owned(),
                    );
                    resp.topics.push(refused(t, duplicate));
                }
                continue;
            }
            let result = match self.create_one(t, req.validate_only, &created) {
                Ok(done) => {
                    if !req.validate_only {
                        created.extend(TopicName::parse(raw).ok());
                    }
                    done
                }
                Err(refusal) => refused(t, refusal),
            };
            resp.topics.push(result);
        }
        resp
    }

    fn create_one(
        &self,
        t: &CreatableTopic,
        validate_only: bool,
        beside: &HashSet<TopicName>,
    ) -> Result<CreatableTopicResult, Refusal> {
        let raw: &str = &t.name;
        let name = TopicName::parse(raw)
            .map_err(|e| (ResponseError::InvalidTopicException, name_message(e, raw)))?;
        if self.meta.topic(&name).is_some() {
            return Err(exists(raw));
        }
        if let Some(c) = self.meta.collision(&name, beside) {
            return Err((ResponseError::InvalidTopicException, collision_message(&c)));
        }
        let partitions = self.partitions_requested(t)?;
        let config = config_map(t);
        topic_config::validate(&config).map_err(|why| (ResponseError::InvalidConfig, why))?;
        let limit = self.config.max_partitions;
        if partitions.get() > limit {
            let why = format!(
                "{} partitions is above this broker's limit of {limit} (--max-partitions)",
                partitions.get()
            );
            return Err((ResponseError::InvalidPartitions, why));
        }
        let topic = if validate_only {
            // Kafka answers a validation with an id it made up for the topic that was not created.
            let id = random_topic_id().map_err(storage_fault)?;
            Topic {
                name,
                id,
                partitions,
                config,
            }
        } else {
            let made = self.make_topic(&name, partitions, config, beside);
            made.map(|t| (*t).clone()).map_err(made_wrong)?
        };
        Ok(created(
            &topic,
            &topic_config::effective(&self.config.defaults, &topic.config),
        ))
    }

    /// How many partitions the request asks for: its manual assignment's length, or its count (-1 is the broker's
    /// default).
    fn partitions_requested(&self, t: &CreatableTopic) -> Result<PartitionCount, Refusal> {
        let invalid = |m: &str| (ResponseError::InvalidRequest, m.to_owned());
        if !t.assignments.is_empty() {
            if t.replication_factor != -1 {
                return Err(invalid(
                    "A manual partition assignment was specified, but replication factor was not set to -1.",
                ));
            }
            if t.num_partitions != -1 {
                return Err(invalid(
                    "A manual partition assignment was specified, but numPartitions was not set to -1.",
                ));
            }
            return self.assigned_partitions(t);
        }
        if t.replication_factor == 0 || t.replication_factor < -1 {
            let why = "Replication factor must be larger than 0, or -1 to use the default value.";
            return Err((ResponseError::InvalidReplicationFactor, why.to_owned()));
        }
        // Greater than one is accepted and answered as one: a development broker has one node, and clients
        // (Connect, Streams) ask for three by default.
        let count = if t.num_partitions == -1 {
            self.config.default_partitions
        } else {
            t.num_partitions
        };
        PartitionCount::new(count).ok_or_else(|| {
            let why = "Number of partitions was set to an invalid non-positive value.";
            (ResponseError::InvalidPartitions, why.to_owned())
        })
    }

    fn assigned_partitions(&self, t: &CreatableTopic) -> Result<PartitionCount, Refusal> {
        let bad = |m: String| (ResponseError::InvalidReplicaAssignment, m);
        let mut replicas = HashMap::new();
        for a in &t.assignments {
            if replicas.insert(a.partition_index, &a.broker_ids).is_some() {
                return Err(bad(format!(
                    "Partition {} is assigned more than once.",
                    a.partition_index
                )));
            }
        }
        let count = i32::try_from(t.assignments.len()).unwrap_or(i32::MAX);
        for p in 0..count {
            let Some(brokers) = replicas.get(&p) else {
                return Err(bad(
                    "partitions should be a consecutive 0-based integer sequence".to_owned(),
                ));
            };
            if brokers.is_empty() {
                return Err(bad(format!(
                    "The manual partition assignment includes an empty replica list for partition {p}."
                )));
            }
            let mut seen = HashSet::new();
            for b in brokers.iter() {
                if b.0 != self.config.node_id {
                    return Err(bad(format!(
                        "The manual partition assignment includes broker {}, but no such broker is registered.",
                        b.0
                    )));
                }
                if !seen.insert(b.0) {
                    return Err(bad(format!(
                        "The manual partition assignment includes the broker {} more than once.",
                        b.0
                    )));
                }
            }
        }
        PartitionCount::new(count).ok_or_else(|| {
            bad("partitions should be a consecutive 0-based integer sequence".to_owned())
        })
    }
}

fn exists(raw: &str) -> Refusal {
    (
        ResponseError::TopicAlreadyExists,
        format!("Topic '{raw}' already exists."),
    )
}

fn config_map(t: &CreatableTopic) -> BTreeMap<String, String> {
    let set = t
        .configs
        .iter()
        .filter_map(|c| Some((c.name.to_string(), c.value.as_ref()?.to_string())));
    set.collect()
}

/// Kafka's words for a name it will not have.
pub(super) fn name_message(e: TopicNameError, name: &str) -> String {
    let why = match e {
        TopicNameError::Empty => "the empty string is not allowed".to_owned(),
        TopicNameError::Reserved => format!("'{name}' is not allowed"),
        TopicNameError::TooLong => format!(
            "the length of '{name}' is longer than the max allowed length {}",
            kafka_wire::ids::MAX_TOPIC_NAME_LEN
        ),
        _ => format!(
            "'{name}' contains one or more characters other than ASCII alphanumerics, '.', '_' and '-'"
        ),
    };
    format!("Topic name is invalid: {why}")
}

fn collision_message(c: &Collision) -> String {
    let base = format!(
        "Topic '{}' collides with existing topic: {}",
        c.name, c.existing
    );
    if c.case_only {
        format!(
            "{base} (names that differ only in letter case would share a directory on a case-insensitive file system)"
        )
    } else {
        base
    }
}

fn storage_fault(e: impl std::fmt::Display) -> Refusal {
    (ResponseError::KafkaStorageError, e.to_string())
}

/// What went wrong making a topic, as the code and words a client is given.
fn made_wrong(e: BrokerError) -> Refusal {
    match e {
        BrokerError::Meta(MetaError::TopicExists(name)) => exists(name.as_str()),
        BrokerError::Meta(MetaError::Collides(c)) => {
            (ResponseError::InvalidTopicException, collision_message(&c))
        }
        other => storage_fault(other),
    }
}

fn refused(t: &CreatableTopic, (error, message): Refusal) -> CreatableTopicResult {
    let mut r = CreatableTopicResult::default();
    r.name = t.name.clone();
    r.error_code = error.code();
    r.error_message = Some(StrBytes::from_string(message));
    r.num_partitions = -1;
    r.replication_factor = -1;
    // Never null: the Java admin client reads it without a check.
    r.configs = Some(Vec::new());
    r
}

fn created(topic: &Topic, configs: &[Entry]) -> CreatableTopicResult {
    let mut r = CreatableTopicResult::default();
    r.name = kafka_wire::messages::TopicName(StrBytes::from_string(topic.name.to_string()));
    r.topic_id = topic.id.to_uuid();
    // Null, not empty: the schema's default is an empty string.
    r.error_message = None;
    r.num_partitions = topic.partitions.get();
    r.replication_factor = 1;
    r.configs = Some(configs.iter().map(config_entry).collect());
    r
}

fn config_entry(e: &Entry) -> CreatableTopicConfigs {
    let mut c = CreatableTopicConfigs::default();
    c.name = StrBytes::from_static_str(e.name);
    c.value = Some(StrBytes::from_string(e.value.clone()));
    c.config_source = e.source.code();
    c
}
