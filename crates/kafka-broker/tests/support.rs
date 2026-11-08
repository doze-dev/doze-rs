//! A broker on a simulated disk, and requests to put to it.
#![allow(
    dead_code,
    unreachable_pub,
    missing_debug_implementations,
    clippy::unwrap_used,
    clippy::panic
)]

use std::path::Path;

use kafka_broker::{Broker, Config, Listener};
use kafka_storage::fs::Fs;
use kafka_storage::sim::SimFs;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::create_topics_request::{
    CreatableReplicaAssignment, CreatableTopic, CreatableTopicConfig,
};
use kafka_wire::messages::create_topics_response::CreatableTopicResult;
use kafka_wire::messages::{ApiKey, BrokerId, CreateTopicsRequest, TopicName};

pub const DATA: &str = "/data";

pub struct Rig {
    pub fs: SimFs,
    pub broker: Broker<SimFs>,
}

pub fn rig() -> Rig {
    let fs = SimFs::new();
    fs.create_dir(Path::new("/")).ok();
    open_on(fs)
}

pub fn open_on(fs: SimFs) -> Rig {
    let broker = Broker::open(fs.clone(), Config::new(DATA.into())).unwrap();
    Rig { fs, broker }
}

pub fn at() -> Listener {
    Listener {
        advertised_host: "127.0.0.1".into(),
        advertised_port: 4242,
    }
}

pub fn head(key: ApiKey, version: i16) -> RequestHead {
    RequestHead {
        key,
        version,
        correlation_id: 1,
        client_id: None,
    }
}

impl Rig {
    /// Puts `request` to the broker at `version` and returns what it answers.
    pub async fn ask(&self, version: i16, request: Request) -> Response {
        let head = head(request.api_key(), version);
        match self.broker.handle(&at(), &head, request).await {
            kafka_broker::Outcome::Respond(response) => response,
            other => panic!("no response: {other:?}"),
        }
    }

    pub async fn create(
        &self,
        topics: Vec<CreatableTopic>,
        validate_only: bool,
    ) -> Vec<CreatableTopicResult> {
        let mut req = CreateTopicsRequest::default();
        req.topics = topics;
        req.validate_only = validate_only;
        let Response::CreateTopics(r) = self.ask(7, Request::CreateTopics(req)).await else {
            panic!("not CreateTopics")
        };
        r.topics
    }

    /// The names in the data directory.
    pub fn dirs(&self) -> Vec<String> {
        let mut names = self.fs.names(Path::new(DATA));
        names.retain(|n| !n.starts_with("meta.redb"));
        names.sort();
        names
    }
}

pub fn wire_name(s: &str) -> TopicName {
    TopicName(StrBytes::from_string(s.to_owned()))
}

pub fn topic(name: &str, partitions: i32) -> CreatableTopic {
    let mut t = CreatableTopic::default();
    t.name = wire_name(name);
    t.num_partitions = partitions;
    t.replication_factor = 1;
    t
}

pub fn with_config(mut t: CreatableTopic, name: &str, value: &str) -> CreatableTopic {
    let mut c = CreatableTopicConfig::default();
    c.name = StrBytes::from_string(name.to_owned());
    c.value = Some(StrBytes::from_string(value.to_owned()));
    t.configs.push(c);
    t
}

pub fn assigned(name: &str, partitions: &[(i32, &[i32])]) -> CreatableTopic {
    let mut t = topic(name, -1);
    t.replication_factor = -1;
    for (index, brokers) in partitions {
        let mut a = CreatableReplicaAssignment::default();
        a.partition_index = *index;
        a.broker_ids = brokers.iter().map(|b| BrokerId(*b)).collect();
        t.assignments.push(a);
    }
    t
}

use kafka_wire::messages::delete_topics_request::DeleteTopicState;
use kafka_wire::messages::delete_topics_response::DeletableTopicResult;
use kafka_wire::messages::metadata_request::MetadataRequestTopic;
use kafka_wire::messages::{DeleteTopicsRequest, MetadataRequest, MetadataResponse};

impl Rig {
    /// Metadata for these topics (None: all), by name, at `version`.
    pub async fn metadata(
        &self,
        version: i16,
        names: Option<&[&str]>,
        allow_auto: bool,
    ) -> MetadataResponse {
        let mut req = MetadataRequest::default();
        req.allow_auto_topic_creation = allow_auto;
        req.topics = names.map(|ns| {
            ns.iter()
                .map(|n| {
                    let mut t = MetadataRequestTopic::default();
                    t.name = Some(wire_name(n));
                    t
                })
                .collect()
        });
        let Response::Metadata(r) = self.ask(version, Request::Metadata(req)).await else {
            panic!("not Metadata")
        };
        r
    }

    /// The names of the topics the broker lists.
    pub async fn topic_names(&self) -> Vec<String> {
        let all = self.metadata(12, None, false).await;
        all.topics
            .iter()
            .map(|t| t.name.as_ref().unwrap().0.to_string())
            .collect()
    }

    pub async fn has_topic(&self, name: &str) -> bool {
        self.topic_names().await.iter().any(|n| n == name)
    }

    /// DeleteTopics by name (v5 form).
    pub async fn delete_by_name(&self, names: &[&str]) -> Vec<DeletableTopicResult> {
        let mut req = DeleteTopicsRequest::default();
        req.topic_names = names.iter().map(|n| wire_name(n)).collect();
        let Response::DeleteTopics(r) = self.ask(5, Request::DeleteTopics(req)).await else {
            panic!("not DeleteTopics")
        };
        r.responses
    }

    /// DeleteTopics by id (v6 form).
    pub async fn delete_by_id(&self, id: kafka_wire::Uuid) -> Vec<DeletableTopicResult> {
        let mut req = DeleteTopicsRequest::default();
        let mut state = DeleteTopicState::default();
        state.topic_id = id;
        req.topics.push(state);
        let Response::DeleteTopics(r) = self.ask(6, Request::DeleteTopics(req)).await else {
            panic!("not DeleteTopics")
        };
        r.responses
    }
}

/// A broker that allows a topic at most `max_partitions` partitions.
pub fn rig_limited(max_partitions: i32) -> Rig {
    let fs = SimFs::new();
    fs.create_dir(Path::new("/")).ok();
    let mut config = Config::new(DATA.into());
    config.max_partitions = max_partitions;
    let broker = Broker::open(fs.clone(), config).unwrap();
    Rig { fs, broker }
}
