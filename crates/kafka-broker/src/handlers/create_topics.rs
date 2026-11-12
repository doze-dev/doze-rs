//! CreateTopics.

use std::collections::{BTreeMap, HashMap, HashSet};

use kafka_auth::authz::operation;
use kafka_meta::{Collision, MetaError, Topic, random_topic_id};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::ids::{TopicName, TopicNameError};
use kafka_wire::messages::create_topics_request::CreatableTopic;
use kafka_wire::messages::create_topics_response::{CreatableTopicConfigs, CreatableTopicResult};
use kafka_wire::messages::{CreateTopicsRequest, CreateTopicsResponse};

use crate::topic_config::{self, Entry};
use crate::{Broker, BrokerError, Caller};

mod assignment;

type Refusal = (ResponseError, String);

/// What a caller that may create a topic may be told of it.
#[derive(Clone, Copy)]
struct Access {
    /// The topic's configs and partition count (`DESCRIBE_CONFIGS` on it).
    configs: bool,
}

impl<F: Fs> Broker<F> {
    /// Creates each topic of the request, and answers once per name. One refused topic does not stop the
    /// others. A topic the caller may not create is answered 29 before it is looked at, so a name that exists, or is
    /// invalid, is not revealed to it; those answers come last.
    pub(crate) async fn create_topics(
        &self,
        caller: &Caller,
        req: &CreateTopicsRequest,
    ) -> CreateTopicsResponse {
        let _one_change_at_a_time = self.topic_admin.lock().await;
        let resp = crate::blocking::run(|| self.create_each(caller, req));
        // Consumer groups that subscribed before a topic existed are waiting for it.
        self.groups.topics_changed().await;
        resp
    }

    /// What `caller` may do with a topic of its request, if it may create it: whether it may also be told the topic's
    /// configs (`DESCRIBE_CONFIGS`). CREATE on the cluster covers every topic; failing that, CREATE on the topic.
    /// Asked before anything else is looked at (a name that exists, or is invalid, is not revealed to a caller that
    /// may not create it): `ControllerApis.createTopics`.
    fn creation_access(&self, caller: &Caller, raw: &str, cluster: bool) -> Option<Access> {
        (cluster || self.topic_allows(caller, raw, operation::CREATE)).then(|| Access {
            configs: self.topic_allows(caller, raw, operation::DESCRIBE_CONFIGS),
        })
    }

    fn create_each(&self, caller: &Caller, req: &CreateTopicsRequest) -> CreateTopicsResponse {
        let cluster = self.cluster_allows(caller, operation::CREATE);
        // The topics refused for want of CREATE come last, after every other answer.
        let mut denied = Vec::new();
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
            let Some(access) = self.creation_access(caller, raw, cluster) else {
                let why = "Authorization failed.".to_owned();
                denied.push(refused(t, (ResponseError::TopicAuthorizationFailed, why)));
                continue;
            };
            let result = match self.create_one(t, req.validate_only, &created, access) {
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
        resp.topics.extend(denied);
        resp
    }

    fn create_one(
        &self,
        t: &CreatableTopic,
        validate_only: bool,
        beside: &HashSet<TopicName>,
        access: Access,
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
        let configs = topic_config::effective(&self.defaults.get(), &topic.config);
        Ok(created(&topic, &configs, access))
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

/// The topic made, as told to a caller. Without DESCRIBE_CONFIGS on it the caller is told only that it was made:
/// the partition count and replication factor are -1, there are no configs and `TopicConfigErrorCode` is 29
/// (`ReplicationControlManager.createTopic`, which answers so whoever has CREATE and not DESCRIBE_CONFIGS).
fn created(topic: &Topic, configs: &[Entry], access: Access) -> CreatableTopicResult {
    let mut r = CreatableTopicResult::default();
    r.name = kafka_wire::messages::TopicName(StrBytes::from_string(topic.name.to_string()));
    r.topic_id = topic.id.to_uuid();
    // Null, not empty: the schema's default is an empty string.
    r.error_message = None;
    if access.configs {
        r.num_partitions = topic.partitions.get();
        r.replication_factor = 1;
        r.configs = Some(configs.iter().map(config_entry).collect());
    } else {
        r.topic_config_error_code = ResponseError::TopicAuthorizationFailed.code();
        r.num_partitions = -1;
        r.replication_factor = -1;
        r.configs = Some(Vec::new());
    }
    r
}

fn config_entry(e: &Entry) -> CreatableTopicConfigs {
    let mut c = CreatableTopicConfigs::default();
    c.name = StrBytes::from_static_str(e.name);
    c.value = Some(StrBytes::from_string(e.value.clone()));
    c.config_source = e.source.code();
    c
}
