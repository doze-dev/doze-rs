//! A broker and its Kafka listener on a fresh data directory, and the console's handle on them: shared by the
//! integration tests that need a real broker (`mod world;`).
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
use kafka_auth::Principal;
use kafka_broker::{Broker, Caller, Config as BrokerConfig, Listener, Outcome};
use kafka_console::Backend;
use kafka_console::base::Base;
use kafka_console::facts::Facts;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_server::{Config as ServerConfig, Server};
use kafka_storage::fs::RealFs;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::add_partitions_to_txn_request::AddPartitionsToTxnTopic;
use kafka_wire::messages::create_topics_request::CreatableTopic;
use kafka_wire::messages::{
    AddPartitionsToTxnRequest, CreateTopicsRequest, InitProducerIdRequest, ProducerId, TopicName,
    TransactionalId,
};
use tokio_util::sync::CancellationToken;

pub struct World {
    pub dir: PathBuf,
    pub broker: Arc<Broker<RealFs>>,
    pub server: Option<kafka_server::Running>,
    pub backend: Backend,
    pub facts: Facts,
}

pub async fn world(name: &str, tweak: impl FnOnce(&mut BrokerConfig, &mut ServerConfig)) -> World {
    let dir = std::env::temp_dir().join(format!("doze-txns-{name}-{}", std::process::id()));
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

impl World {
    pub fn router(&self) -> Router {
        let live = Live::new(self.backend.clone(), self.facts.clone());
        kafka_console::app::console(State::new(
            Source::Live(Box::new(live)),
            Base::root(),
            vec![],
            CancellationToken::new(),
        ))
    }

    pub async fn ask(&self, version: i16, request: Request) -> Response {
        let head = RequestHead {
            key: request.api_key(),
            version,
            correlation_id: 1,
            client_id: Some(StrBytes::from_static_str("test")),
        };
        let caller = Caller::new(Principal::console(), Some("127.0.0.1".parse().unwrap()));
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

    pub async fn create_topic(&self, name: &str) {
        let mut t = CreatableTopic::default();
        t.name = TopicName(StrBytes::from_string(name.to_owned()));
        t.num_partitions = 1;
        t.replication_factor = 1;
        let mut r = CreateTopicsRequest::default();
        r.topics = vec![t];
        let Response::CreateTopics(r) = self.ask(7, Request::CreateTopics(r)).await else {
            panic!()
        };
        assert_eq!(r.topics[0].error_code, 0);
    }

    /// Opens a transaction of `id` on `orders-0` that the coordinator aborts after `timeout_ms`.
    pub async fn open_txn(&self, id: &str, timeout_ms: i32) {
        let mut init = InitProducerIdRequest::default();
        init.transactional_id = Some(TransactionalId(StrBytes::from_string(id.to_owned())));
        init.transaction_timeout_ms = timeout_ms;
        let Response::InitProducerId(p) = self.ask(4, Request::InitProducerId(init)).await else {
            panic!()
        };
        assert_eq!(p.error_code, 0);
        let mut add = AddPartitionsToTxnRequest::default();
        add.v3_and_below_transactional_id = TransactionalId(StrBytes::from_string(id.to_owned()));
        add.v3_and_below_producer_id = ProducerId(p.producer_id.0);
        add.v3_and_below_producer_epoch = p.producer_epoch;
        let mut topic = AddPartitionsToTxnTopic::default();
        topic.name = TopicName(StrBytes::from_static_str("orders"));
        topic.partitions = vec![0];
        add.v3_and_below_topics.push(topic);
        let Response::AddPartitionsToTxn(r) = self.ask(3, Request::AddPartitionsToTxn(add)).await
        else {
            panic!()
        };
        assert_eq!(
            r.results_by_topic_v3_and_below[0].results_by_partition[0].partition_error_code,
            0
        );
    }

    pub async fn stop(mut self) {
        if let Some(s) = self.server.take() {
            s.shutdown().await;
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
