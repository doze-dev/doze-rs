//! Shared by the live tests of the topic writes: a real broker and Kafka listener in this process, the console's pages in
//! front of it, and the protocol requests a test uses to look at what a page did.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use kafka_auth::Principal;
use kafka_broker::{Broker, Caller, Config as BrokerConfig, Listener, Outcome};
use kafka_console::Backend;
use kafka_console::base::Base;
use kafka_console::facts::Facts;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_server::{Config as ServerConfig, Running, Server};
use kafka_storage::fs::RealFs;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request as KRequest, Response};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::create_topics_request::{CreatableTopic, CreatableTopicConfig};
use kafka_wire::messages::{CreateTopicsRequest, TopicName};
use tokio_util::sync::CancellationToken;

use crate::support::{HOST, send};

/// A broker and its Kafka listener on a fresh data directory, and the console's handle on them.
pub struct World {
    pub dir: PathBuf,
    pub broker: Arc<Broker<RealFs>>,
    pub server: Option<Running>,
    pub backend: Backend,
    pub facts: Facts,
}

pub async fn world(name: &str, tweak: impl FnOnce(&mut BrokerConfig, &mut ServerConfig)) -> World {
    let dir = std::env::temp_dir().join(format!("doze-console-w-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut bc = BrokerConfig::new(dir.clone());
    let mut sc = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        ..ServerConfig::default()
    };
    tweak(&mut bc, &mut sc);
    let broker = Arc::new(Broker::open(RealFs, bc).unwrap());
    let server = Server::bind(sc.clone(), Arc::clone(&broker)).await.unwrap();
    let listener: Listener = sc.advertise(server.local_addr());
    let facts = Facts {
        listen: server.local_addr(),
        advertised_host: listener.advertised_host.clone(),
        advertised_port: u16::try_from(listener.advertised_port).unwrap(),
        tls: false,
        sasl: vec![],
        kafka_major: 4,
        allow_hosts: vec![],
        unprotected: false,
    };
    let backend = Backend::new(Arc::clone(&broker), listener, sc.profile);
    World {
        dir,
        broker,
        server: Some(server),
        backend,
        facts,
    }
}

/// A router over a backend (the console's own, or one acting as someone else, or one that keeps a log).
pub fn router_of(w: &World, backend: Backend) -> Router {
    let live = Live::new(backend, w.facts.clone());
    let state = State::new(
        Source::Live(Box::new(live)),
        Base::root(),
        vec![],
        CancellationToken::new(),
    );
    kafka_console::app::router(state)
}

impl World {
    pub fn router(&self) -> Router {
        router_of(self, self.backend.clone())
    }

    /// A request as a client would put it, as the console's principal, at an explicit version.
    pub async fn ask(&self, version: i16, request: KRequest) -> Response {
        self.ask_as(
            Caller::new(Principal::console(), Some("127.0.0.1".parse().unwrap())),
            version,
            request,
        )
        .await
    }

    pub async fn ask_as(&self, caller: Caller, version: i16, request: KRequest) -> Response {
        let head = RequestHead {
            key: request.api_key(),
            version,
            correlation_id: 1,
            client_id: Some(StrBytes::from_static_str("test")),
        };
        let listener = Listener {
            advertised_host: "127.0.0.1".into(),
            advertised_port: 1,
            client_host: "/127.0.0.1".into(),
        };
        match self.broker.handle(&listener, &caller, &head, request).await {
            Outcome::Respond(r) => r,
            other => panic!("{other:?}"),
        }
    }

    /// Makes a topic the way a client would, with these configs.
    pub async fn create_topic(&self, name: &str, partitions: i32, configs: &[(&str, &str)]) {
        let mut t = CreatableTopic::default();
        t.name = TopicName(StrBytes::from_string(name.to_owned()));
        t.num_partitions = partitions;
        t.replication_factor = 1;
        t.configs = configs
            .iter()
            .map(|(k, v)| {
                let mut c = CreatableTopicConfig::default();
                c.name = StrBytes::from_string((*k).to_owned());
                c.value = Some(StrBytes::from_string((*v).to_owned()));
                c
            })
            .collect();
        let mut r = CreateTopicsRequest::default();
        r.topics = vec![t];
        let Response::CreateTopics(r) = self.ask(7, KRequest::CreateTopics(r)).await else {
            panic!()
        };
        assert_eq!(r.topics[0].error_code, 0, "{r:?}");
    }

    /// The topics the broker lists, with their partition counts.
    pub async fn topics(&self) -> Vec<(String, usize)> {
        let Response::Metadata(m) = self
            .ask(12, kafka_console::backend::requests::metadata_all())
            .await
        else {
            panic!()
        };
        let mut out: Vec<_> = m
            .topics
            .iter()
            .map(|t| (t.name.as_ref().unwrap().0.to_string(), t.partitions.len()))
            .collect();
        out.sort();
        out
    }

    /// An offset of a partition: `timestamp` -2 earliest, -1 latest.
    pub async fn offset(&self, topic: &str, partition: i32, timestamp: i64) -> i64 {
        let req =
            kafka_console::backend::requests::forms::offsets_at(topic, &[partition], timestamp, 0);
        let Response::ListOffsets(r) = self.ask(7, req).await else {
            panic!()
        };
        let p = &r.topics[0].partitions[0];
        assert_eq!(p.error_code, 0, "{p:?}");
        p.offset
    }

    /// The value of a topic config, as DescribeConfigs says it.
    pub async fn config(&self, topic: &str, key: &str) -> Option<String> {
        let req = kafka_console::backend::requests::forms::topic_configs(topic, &[key]);
        let Response::DescribeConfigs(r) = self.ask(4, req).await else {
            panic!()
        };
        r.results[0]
            .configs
            .iter()
            .find(|c| c.name.as_str() == key)
            .and_then(|c| c.value.as_ref().map(ToString::to_string))
    }

    /// The bytes of the batches a Fetch returns from `offset`.
    pub async fn fetch(&self, topic: &str, partition: i32, offset: i64) -> Vec<u8> {
        use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
        let mut p = FetchPartition::default();
        p.partition = partition;
        p.current_leader_epoch = -1;
        p.fetch_offset = offset;
        p.last_fetched_epoch = -1;
        p.partition_max_bytes = 1 << 20;
        let mut t = FetchTopic::default();
        t.topic = TopicName(StrBytes::from_string(topic.to_owned()));
        t.partitions = vec![p];
        let mut r = kafka_wire::messages::FetchRequest::default();
        r.replica_id = kafka_wire::messages::BrokerId(-1);
        r.max_wait_ms = 0;
        r.min_bytes = 1;
        r.max_bytes = 1 << 20;
        r.session_epoch = -1;
        r.topics = vec![t];
        let Response::Fetch(r) = self.ask(12, KRequest::Fetch(r)).await else {
            panic!()
        };
        let part = &r.responses[0].partitions[0];
        assert_eq!(part.error_code, 0, "{part:?}");
        part.records
            .as_ref()
            .map(|b| b.to_vec())
            .unwrap_or_default()
    }

    pub async fn stop(mut self) {
        if let Some(s) = self.server.take() {
            s.shutdown().await;
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Percent-encodes a form value.
pub fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_') {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// The body of a form with these fields.
pub fn body(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// A POST of a form that proves it came from the console's own pages.
pub fn post_form(path: &str, fields: &[(&str, &str)]) -> Request<Body> {
    crate::support::post(path, &body(fields))
}

/// The same, as a script asks for it: the body alone.
pub fn fragment_get(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-fragment", "1")
        .body(Body::empty())
        .unwrap()
}

/// Sends a request and returns the status and the text.
pub async fn page(app: &Router, req: Request<Body>) -> (StatusCode, String) {
    let (res, text) = send(app, req).await;
    (res.status(), text)
}
