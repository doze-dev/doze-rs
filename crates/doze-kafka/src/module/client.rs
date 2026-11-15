//! The little Kafka client the module face speaks to its own broker with: one connection to the main backend, requests
//! sent one at a time. The broker is a single node, so nothing here follows the addresses Metadata advertises (they are
//! the client-facing ones, which only doze's proxy serves): every request goes down the one connection, as the Go
//! module's dialer did.
//!
//! The versions are fixed, each one inside what every Kafka major the broker can stand for (1 to 4) offers
//! (`kafka-wire`'s `api.rs` table), so the module needs no ApiVersions round before it can talk.

use std::collections::BTreeMap;
use std::time::Duration;

use bytes::{Buf, Bytes, BytesMut};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::Instance;
use doze_module::{Result, help};
use kafka_protocol::messages::create_partitions_request::CreatePartitionsTopic;
use kafka_protocol::messages::create_topics_request::{CreatableTopic, CreatableTopicConfig};
use kafka_protocol::messages::describe_configs_request::DescribeConfigsResource;
use kafka_protocol::messages::{
    AlterConfigsRequest, AlterConfigsResponse, ApiKey, ApiVersionsRequest, ApiVersionsResponse,
    CreatePartitionsRequest, CreatePartitionsResponse, CreateTopicsRequest, CreateTopicsResponse,
    DescribeConfigsRequest, DescribeConfigsResponse, MetadataRequest, MetadataResponse,
    RequestHeader, ResponseHeader, TopicName,
};
use kafka_protocol::protocol::{Decodable, Encodable, StrBytes};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod data;

/// How long one request may take before the broker is taken to be gone (a fetch waits at most `FETCH_WAIT` of it).
const CALL_LIMIT: Duration = Duration::from_secs(30);
/// How long a fetch waits for records to arrive before it answers with none.
pub(crate) const FETCH_WAIT: Duration = Duration::from_millis(500);
/// The largest answer read (a fetch asks for at most `FETCH_BYTES`).
const MOST: usize = 16 << 20;
const FETCH_BYTES: i32 = 1 << 20;
/// Kafka's code for a topic that already exists.
const TOPIC_ALREADY_EXISTS: i16 = 36;
/// The resource type of a topic in DescribeConfigs and AlterConfigs.
const TOPIC_RESOURCE: i8 = 2;
/// The config source of a config set on the topic itself (DescribeConfigs v1).
const DYNAMIC_TOPIC_CONFIG: i8 = 1;

/// A topic as Metadata reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TopicInfo {
    pub(crate) name: String,
    pub(crate) partitions: Vec<i32>,
}

/// A consumer group as DescribeGroups reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GroupInfo {
    pub(crate) name: String,
    pub(crate) state: String,
    pub(crate) members: usize,
}

/// A record as a fetch returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fetched {
    pub(crate) partition: i32,
    pub(crate) offset: i64,
    pub(crate) timestamp: i64,
    pub(crate) key: Vec<u8>,
    pub(crate) value: Vec<u8>,
}

fn unavailable(what: &str, e: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Unavailable, format!("{what}: {e}"))
}

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

fn topic_name(s: &str) -> TopicName {
    TopicName(text(s))
}

/// The broker's answer to one topic: its error code and message, as an error that names the topic.
fn refused(topic: &str, what: &str, code: i16, message: Option<&StrBytes>) -> Error {
    let why = message.map_or_else(|| format!("Kafka error {code}"), |m| m.to_string());
    Error::new(ErrorKind::Invalid, format!("topic {topic}: {what}: {why}"))
}

/// One connection to the broker's main backend.
pub(crate) struct Client {
    stream: doze_os::net::Stream,
    correlation: i32,
}

impl Client {
    /// Connects to the instance's broker; `Unavailable` if it is not running.
    pub(crate) async fn connect(instance: &Instance) -> Result<Client> {
        Ok(Client {
            stream: help::dial_backend(instance).await?,
            correlation: 0,
        })
    }

    async fn call<Q: Encodable, A: Decodable>(
        &mut self,
        key: ApiKey,
        version: i16,
        request: &Q,
    ) -> Result<A> {
        self.correlation = self.correlation.wrapping_add(1);
        let mut header = RequestHeader::default();
        header.request_api_key = key as i16;
        header.request_api_version = version;
        header.correlation_id = self.correlation;
        header.client_id = Some(text("doze-kafka-module"));
        let mut payload = BytesMut::new();
        let encoded = header
            .encode(&mut payload, key.request_header_version(version))
            .and_then(|()| request.encode(&mut payload, version));
        encoded.map_err(|e| Error::new(ErrorKind::Internal, format!("{key:?}: {e}")))?;
        let len = u32::try_from(payload.len())
            .map_err(|_| Error::new(ErrorKind::Internal, "a request too large to send"))?;
        let exchange = async {
            self.stream.write_all(&len.to_be_bytes()).await?;
            self.stream.write_all(&payload).await?;
            self.stream.flush().await?;
            let mut size = [0_u8; 4];
            self.stream.read_exact(&mut size).await?;
            let size = usize::try_from(u32::from_be_bytes(size)).unwrap_or(usize::MAX);
            if size > MOST {
                return Err(std::io::Error::other(format!(
                    "an answer of {size} bytes is too large"
                )));
            }
            let mut body = vec![0_u8; size];
            self.stream.read_exact(&mut body).await?;
            Ok(Bytes::from(body))
        };
        let mut body = tokio::time::timeout(CALL_LIMIT, exchange)
            .await
            .map_err(|_| unavailable("the broker", "it did not answer in 30s"))?
            .map_err(|e| unavailable("the broker", e))?;
        let decoded = ResponseHeader::decode(&mut body, key.response_header_version(version))
            .and_then(|h| Ok((h, A::decode(&mut body, version)?)));
        let (h, answer) = decoded.map_err(|e| {
            Error::new(
                ErrorKind::Internal,
                format!("{key:?}: an answer that cannot be read: {e}"),
            )
        })?;
        if h.correlation_id != self.correlation || body.has_remaining() {
            return Err(Error::new(
                ErrorKind::Internal,
                format!("{key:?}: an answer that is not to this request"),
            ));
        }
        Ok(answer)
    }

    /// Asks the broker which APIs it serves: the health check.
    pub(crate) async fn ping(&mut self) -> Result<()> {
        let a: ApiVersionsResponse = self
            .call(ApiKey::ApiVersions, 0, &ApiVersionsRequest::default())
            .await?;
        if a.error_code != 0 {
            return Err(Error::new(
                ErrorKind::Unavailable,
                format!("ApiVersions: Kafka error {}", a.error_code),
            ));
        }
        Ok(())
    }

    /// Every topic but the broker's internal ones, by name, with its partitions in order. Asking for all topics (not
    /// for some by name) never creates one, whatever `auto_create_topics` says.
    pub(crate) async fn topics(&mut self) -> Result<BTreeMap<String, TopicInfo>> {
        let mut q = MetadataRequest::default();
        q.topics = None;
        let a: MetadataResponse = self.call(ApiKey::Metadata, 1, &q).await?;
        let mut out = BTreeMap::new();
        for t in a.topics.into_iter().filter(|t| !t.is_internal) {
            let Some(name) = t.name.map(|n| n.0.to_string()) else {
                continue;
            };
            let mut partitions: Vec<i32> = t.partitions.iter().map(|p| p.partition_index).collect();
            partitions.sort_unstable();
            out.insert(name.clone(), TopicInfo { name, partitions });
        }
        Ok(out)
    }

    /// Creates a topic. One that already exists (a client made it first) is not an error.
    pub(crate) async fn create_topic(
        &mut self,
        name: &str,
        partitions: i32,
        configs: &BTreeMap<String, String>,
    ) -> Result<bool> {
        let mut t = CreatableTopic::default();
        t.name = topic_name(name);
        t.num_partitions = partitions;
        t.replication_factor = 1;
        for (k, v) in configs {
            let mut c = CreatableTopicConfig::default();
            c.name = text(k);
            c.value = Some(text(v));
            t.configs.push(c);
        }
        let mut q = CreateTopicsRequest::default();
        q.topics.push(t);
        q.timeout_ms = 30_000;
        let a: CreateTopicsResponse = self.call(ApiKey::CreateTopics, 2, &q).await?;
        let Some(r) = a.topics.first() else {
            return Err(Error::new(
                ErrorKind::Internal,
                format!("topic {name}: CreateTopics answered nothing"),
            ));
        };
        match r.error_code {
            0 => Ok(true),
            TOPIC_ALREADY_EXISTS => Ok(false),
            code => Err(refused(
                name,
                "cannot be created",
                code,
                r.error_message.as_ref(),
            )),
        }
    }

    /// Raises a topic's partition count to `count`.
    pub(crate) async fn add_partitions(&mut self, name: &str, count: i32) -> Result<()> {
        let mut t = CreatePartitionsTopic::default();
        t.name = topic_name(name);
        t.count = count;
        t.assignments = None;
        let mut q = CreatePartitionsRequest::default();
        q.topics.push(t);
        q.timeout_ms = 30_000;
        let a: CreatePartitionsResponse = self.call(ApiKey::CreatePartitions, 0, &q).await?;
        match a.results.first() {
            Some(r) if r.error_code != 0 => Err(refused(
                name,
                "cannot have its partitions raised",
                r.error_code,
                r.error_message.as_ref(),
            )),
            _ => Ok(()),
        }
    }

    /// The configs set on a topic itself (not the broker's defaults it falls back to).
    pub(crate) async fn topic_configs(&mut self, name: &str) -> Result<BTreeMap<String, String>> {
        let mut r = DescribeConfigsResource::default();
        r.resource_type = TOPIC_RESOURCE;
        r.resource_name = text(name);
        r.configuration_keys = None;
        let mut q = DescribeConfigsRequest::default();
        q.resources.push(r);
        let a: DescribeConfigsResponse = self.call(ApiKey::DescribeConfigs, 1, &q).await?;
        let Some(res) = a.results.into_iter().next() else {
            return Ok(BTreeMap::new());
        };
        if res.error_code != 0 {
            return Err(refused(
                name,
                "its configs cannot be read",
                res.error_code,
                res.error_message.as_ref(),
            ));
        }
        Ok(res
            .configs
            .into_iter()
            .filter(|c| c.config_source == DYNAMIC_TOPIC_CONFIG)
            .filter_map(|c| Some((c.name.to_string(), c.value?.to_string())))
            .collect())
    }

    /// Replaces the configs set on a topic with `configs` (AlterConfigs, which every Kafka major offers: the caller
    /// passes the topic's own configs with its changes laid over them, so nothing it did not mean to change is lost).
    pub(crate) async fn set_topic_configs(
        &mut self,
        name: &str,
        configs: &BTreeMap<String, String>,
    ) -> Result<()> {
        use kafka_protocol::messages::alter_configs_request::{
            AlterConfigsResource, AlterableConfig,
        };
        let mut r = AlterConfigsResource::default();
        r.resource_type = TOPIC_RESOURCE;
        r.resource_name = text(name);
        for (k, v) in configs {
            let mut c = AlterableConfig::default();
            c.name = text(k);
            c.value = Some(text(v));
            r.configs.push(c);
        }
        let mut q = AlterConfigsRequest::default();
        q.resources.push(r);
        let a: AlterConfigsResponse = self.call(ApiKey::AlterConfigs, 0, &q).await?;
        match a.responses.first() {
            Some(r) if r.error_code != 0 => Err(refused(
                name,
                "its configs cannot be set",
                r.error_code,
                r.error_message.as_ref(),
            )),
            _ => Ok(()),
        }
    }
}
