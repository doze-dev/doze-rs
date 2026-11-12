//! The console over a real broker in the same process: its pages are made from the broker's own answers, through
//! `Broker::handle`, as the principal `User:__console`; and the advertised-listener check dials a real listener.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::StatusCode;
use kafka_auth::Principal;
use kafka_broker::{Broker, Caller, Config as BrokerConfig, Listener, Outcome};
use kafka_console::base::Base;
use kafka_console::facts::Facts;
use kafka_console::probe::Prober;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_console::{Backend, model::Verdict};
use kafka_server::{Config as ServerConfig, Running, Server};
use kafka_storage::fs::RealFs;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::add_partitions_to_txn_request::AddPartitionsToTxnTopic;
use kafka_wire::messages::create_topics_request::CreatableTopic;
use kafka_wire::messages::{
    AddPartitionsToTxnRequest, ApiKey, CreateTopicsRequest, InitProducerIdRequest, ProducerId,
    TopicName, TransactionalId,
};
use support::{get, send};
use tokio_util::sync::CancellationToken;

/// A broker and its Kafka listener on a fresh data directory, and the console's handle on them.
struct World {
    dir: PathBuf,
    broker: Arc<Broker<RealFs>>,
    server: Option<Running>,
    backend: Backend,
    facts: Facts,
}

async fn world(name: &str, tweak: impl FnOnce(&mut BrokerConfig, &mut ServerConfig)) -> World {
    let dir = std::env::temp_dir().join(format!("doze-console-{name}-{}", std::process::id()));
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
    fn router(&self) -> Router {
        let live = Live::new(self.backend.clone(), self.facts.clone());
        let state = State::new(
            Source::Live(Box::new(live)),
            Base::root(),
            vec![],
            CancellationToken::new(),
        );
        kafka_console::app::router(state)
    }

    /// A request as a client would put it, at an explicit version.
    async fn ask(&self, version: i16, request: Request) -> Response {
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

    async fn create_topic(&self, name: &str) {
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

    async fn stop(mut self) {
        if let Some(s) = self.server.take() {
            s.shutdown().await;
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn a_broker_with_no_topics_is_the_quiet_home_and_with_one_it_is_the_steady_one() {
    let w = world("home", |_, _| {}).await;
    let app = w.router();
    let (res, body) = send(&app, get("/")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body.contains("<h1>Your broker is up</h1>"), "{body:.300}");
    assert!(body.contains("Started on a new data directory."), "{body}");
    assert!(body.contains(&w.facts.listen.to_string()));
    w.create_topic("orders").await;
    let (_, body) = send(&app, get("/")).await;
    assert!(body.contains("<h1>Home</h1>"), "{body:.300}");
    assert!(
        body.contains(r#"<span class="k">Topics</span><span class="v" title="1">1</span>"#),
        "{body}"
    );
    assert!(body.contains("Nothing needs you."));
    w.stop().await;
}

#[tokio::test]
async fn the_console_reads_a_broker_that_refuses_everyone_else_because_it_is_a_super_user() {
    let w = world("deny", |b, _| b.acl_default_allow = false).await;
    w.create_topic("orders").await;
    let app = w.router();
    let (_, body) = send(&app, get("/")).await;
    assert!(
        body.contains(r#"<span class="k">Topics</span><span class="v" title="1">1</span>"#),
        "{body}"
    );
    assert!(
        body.contains("Open · ACLs"),
        "the chip says ACLs deny by default"
    );
    assert!(body.contains("ACLs deny by default"), "{body}");
    w.stop().await;
}

#[tokio::test]
async fn an_open_transaction_close_to_its_timeout_is_what_needs_someone() {
    let w = world("txn", |_, _| {}).await;
    w.create_topic("orders").await;
    let mut init = InitProducerIdRequest::default();
    init.transactional_id = Some(TransactionalId(StrBytes::from_static_str("orders-eos")));
    init.transaction_timeout_ms = 900;
    let Response::InitProducerId(p) = w.ask(4, Request::InitProducerId(init)).await else {
        panic!()
    };
    assert_eq!(p.error_code, 0);
    let mut add = AddPartitionsToTxnRequest::default();
    add.v3_and_below_transactional_id = TransactionalId(StrBytes::from_static_str("orders-eos"));
    add.v3_and_below_producer_id = ProducerId(p.producer_id.0);
    add.v3_and_below_producer_epoch = p.producer_epoch;
    let mut topic = AddPartitionsToTxnTopic::default();
    topic.name = TopicName(StrBytes::from_static_str("orders"));
    topic.partitions = vec![0];
    add.v3_and_below_topics.push(topic);
    let Response::AddPartitionsToTxn(r) = w.ask(3, Request::AddPartitionsToTxn(add)).await else {
        panic!()
    };
    assert_eq!(
        r.results_by_topic_v3_and_below[0].results_by_partition[0].partition_error_code,
        0
    );
    let app = w.router();
    // Just opened: not close.
    let (_, body) = send(&app, get("/")).await;
    assert!(!body.contains("1 thing needs you"), "{body}");
    // Two thirds of 900 ms is 600 ms.
    tokio::time::sleep(Duration::from_millis(700)).await;
    let (_, body) = send(&app, get("/")).await;
    assert!(
        body.contains("1 thing needs you: a transaction is close to its timeout."),
        "{body}"
    );
    assert!(
        body.contains("<span class=\"mono\">orders-eos</span>"),
        "{body}"
    );
    assert!(
        body.contains(&format!(
            "producer {}, epoch {}",
            p.producer_id.0, p.producer_epoch
        )),
        "{body}"
    );
    assert!(body.contains("<span class=\"mono\">orders-0</span>"));
    assert!(body.contains("/topics/orders/transactions/"));
    let (_, frag) = send(&app, get("/fragments/home-live")).await;
    assert!(
        frag.contains("1 thing needs you"),
        "the fragment says what the page says"
    );
    w.stop().await;
}

#[tokio::test]
async fn the_check_passes_against_a_real_listener_and_the_page_says_so() {
    let w = world("probe", |_, _| {}).await;
    let p = Prober::new(w.facts.clone(), w.backend.clone());
    let v = p.run().await;
    assert_eq!(v.verdict, Verdict::Reachable, "{v:?}");
    assert_eq!(v.rules.len(), 3);
    assert!(v.same);
    assert!(v.rules.iter().all(|r| r.ok), "{v:?}");
    assert_eq!(v.metadata_version, 12);
    let app = w.router();
    let (_, body) = send(&app, get("/connect/advertised")).await;
    assert!(
        body.contains("Advertised host and port equal the listener"),
        "{body}"
    );
    assert!(body.contains("<span class=\"st ok\">Reachable</span>"));
    w.stop().await;
}

#[tokio::test]
async fn a_deliberately_wrong_advertised_address_fails_the_third_rule_in_words() {
    // Advertised on a port nothing listens on: Metadata says so truthfully, and the dial back is refused.
    let closed = {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    };
    let w = world("wrong", |_, s| s.advertised_port = Some(closed)).await;
    let v = Prober::new(w.facts.clone(), w.backend.clone()).run().await;
    assert_eq!(v.verdict, Verdict::NotReachable, "{v:?}");
    assert!(!v.same);
    assert!(v.rules[0].ok && v.rules[1].ok && !v.rules[2].ok, "{v:?}");
    assert_eq!(v.rules[2].title, "That address is not reachable");
    assert_eq!(v.rules[2].value, "failed");
    let app = w.router();
    let (_, body) = send(&app, get("/connect/advertised")).await;
    assert!(
        body.contains("<span class=\"st bad\">Not reachable</span>"),
        "{body:.200}"
    );
    assert!(body.contains(&format!("127.0.0.1:{closed}")));
    assert!(body.contains("connection refused"), "{body}");
    assert!(body.contains("differ from the listener"), "{body}");
    w.stop().await;
}

#[tokio::test]
async fn a_name_that_cannot_be_resolved_is_not_reachable_and_the_check_stays_inside_its_budget() {
    let w = world("name", |_, s| {
        s.advertised_host = Some("no-such-host.invalid".to_owned())
    })
    .await;
    let started = std::time::Instant::now();
    let v = Prober::new(w.facts.clone(), w.backend.clone()).run().await;
    assert!(
        started.elapsed() < Duration::from_millis(1700),
        "{:?}",
        started.elapsed()
    );
    assert!(
        matches!(v.verdict, Verdict::NotReachable | Verdict::NotChecked),
        "{v:?}"
    );
    if v.verdict == Verdict::NotReachable {
        assert!(!v.rules[2].ok && v.rules[2].detail.iter().any(|s| matches!(s, kafka_console::model::Span::Mono(a) if a.starts_with("no-such-host.invalid"))), "{v:?}");
    }
    w.stop().await;
}

#[tokio::test]
async fn a_listener_that_is_gone_fails_the_first_rule_and_a_result_is_reused_for_thirty_seconds() {
    let mut w = world("gone", |_, _| {}).await;
    let p = Prober::new(w.facts.clone(), w.backend.clone());
    assert_eq!(p.run().await.verdict, Verdict::Reachable);
    w.server.take().unwrap().shutdown().await;
    // The last result is fresh, so page load shows it without dialling again.
    assert_eq!(p.cached().await.verdict, Verdict::Reachable);
    // Asking again dials, and the listener is gone.
    let v = p.run().await;
    assert_eq!(v.verdict, Verdict::NotReachable, "{v:?}");
    assert!(!v.rules[0].ok);
    assert_eq!(v.rules[0].title, "The listener does not answer");
    w.stop().await;
}

#[tokio::test]
async fn the_consoles_own_requests_name_it_and_are_answered_at_the_highest_version() {
    let w = world("head", |_, _| {}).await;
    assert_eq!(w.backend.version(ApiKey::Metadata), 12);
    assert_eq!(w.backend.caller().principal().as_str(), "User:__console");
    let Response::Metadata(m) = w
        .backend
        .call(kafka_console::backend::requests::metadata_brokers())
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(m.brokers.len(), 1);
    assert!(m.topics.is_empty());
    w.stop().await;
}
