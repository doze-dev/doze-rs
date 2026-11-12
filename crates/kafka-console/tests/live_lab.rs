//! The Lab over a real broker with traffic: a SLOW rule delays a Fetch, a FAIL rule answers Produce with its code and writes nothing, a
//! DROP closes the connection, an expiry runs a member out through the group's own timer path, a rebalance goes through the group
//! machine, Compact now runs the cleaner's real round; every request a rule touched carries its tag on the wire; a caller without
//! the right is refused; hostile names stay text.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_log;
mod support_write;

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use kafka_auth::Principal;
use kafka_broker::{Caller, LabOutcome};
use kafka_console::Recorder;
use kafka_console::gather::lab as glab;
use kafka_console::lab::Lab;
use kafka_console::model::lab::{LabFields, LabKind, LabMade, LabOnceDone};
use kafka_console::model::live::LabDid;
use kafka_protocol::protocol::{Decodable, Encodable};
use kafka_server::fault::FaultTable;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::join_group_request::JoinGroupRequestProtocol;
use kafka_wire::messages::*;
use kafka_wire::testing::{framed, request_payload};
use support_log::{keyed, records};
use support_write::{World, world};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct Lw {
    w: World,
    lab: Lab,
    rec: Arc<Recorder>,
}

async fn lab_world(name: &str) -> Lw {
    let table = FaultTable::new();
    let rec = Recorder::new();
    let (t, r) = (Arc::clone(&table), Arc::clone(&rec));
    let w = world(name, move |bc, sc| {
        sc.observer = Some(r);
        sc.fault = Some(t);
        bc.acl_default_allow = true;
    })
    .await;
    w.create_topic("orders", 1, &[]).await;
    Lw {
        w,
        lab: Lab::new(table),
        rec,
    }
}

/// A raw client whose calls may meet a closed connection.
struct Raw {
    stream: TcpStream,
    id: i32,
    client: &'static str,
}

impl Raw {
    async fn connect(w: &World, client: &'static str) -> Self {
        let addr = w.server.as_ref().unwrap().local_addr();
        Self {
            stream: TcpStream::connect(addr).await.unwrap(),
            id: 0,
            client,
        }
    }

    async fn try_call<R: Encodable>(
        &mut self,
        key: ApiKey,
        version: i16,
        body: &R,
    ) -> Option<Bytes> {
        self.id += 1;
        let req = framed(&request_payload(
            key,
            version,
            self.id,
            Some(self.client),
            body,
        ));
        let _ = self.stream.write_all(&req).await;
        let mut len = [0; 4];
        tokio::time::timeout(Duration::from_secs(5), self.stream.read_exact(&mut len))
            .await
            .ok()?
            .ok()?;
        let mut payload = vec![0; u32::from_be_bytes(len) as usize];
        self.stream.read_exact(&mut payload).await.ok()?;
        Some(Bytes::from(payload))
    }
}

fn body<T: Decodable>(key: ApiKey, version: i16, mut payload: Bytes) -> T {
    let _ = kafka_protocol::messages::ResponseHeader::decode(
        &mut payload,
        key.response_header_version(version),
    )
    .unwrap();
    T::decode(&mut payload, version).unwrap()
}

fn s(t: &str) -> StrBytes {
    StrBytes::from_string(t.to_owned())
}

fn produce_req(topic: &str) -> ProduceRequest {
    let mut p = kafka_wire::messages::produce_request::PartitionProduceData::default();
    p.index = 0;
    p.records = Some(records(2));
    let mut t = kafka_wire::messages::produce_request::TopicProduceData::default();
    t.name = TopicName(s(topic));
    t.partition_data.push(p);
    let mut r = ProduceRequest::default();
    r.acks = -1;
    r.timeout_ms = 5000;
    r.topic_data.push(t);
    r
}

fn fetch_req(topic: &str) -> FetchRequest {
    let mut p = kafka_wire::messages::fetch_request::FetchPartition::default();
    p.partition = 0;
    p.partition_max_bytes = 1 << 20;
    let mut t = kafka_wire::messages::fetch_request::FetchTopic::default();
    t.topic = TopicName(s(topic));
    t.partitions.push(p);
    let mut r = FetchRequest::default();
    r.max_wait_ms = 0;
    r.max_bytes = 1 << 20;
    r.topics.push(t);
    r
}

fn fields(pairs: &[(&str, &str)]) -> LabFields {
    let f: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    kafka_console::lab::form::fields_of(&f)
}

async fn make(l: &Lw, kind: LabKind, pairs: &[(&str, &str)]) -> u32 {
    match glab::create(&l.lab, &l.w.backend, Some(&l.rec), kind, fields(pairs)).await {
        LabMade::Made(id) => id,
        LabMade::Invalid(v) => panic!("{v:?}"),
    }
}

fn wire_hits(l: &Lw, rule: u32) -> Vec<kafka_console::model::WireRow> {
    l.rec.lab_hits(rule, 50).0
}

/// LAB-14: a SLOW rule delays the Fetch it matches by its delay and by nothing else; the wire tags the row with the rule and says how
/// much it added; clearing the rule makes the next Fetch fast and untagged.
#[tokio::test]
async fn a_slow_rule_delays_a_fetch_and_the_wire_says_so() {
    let l = lab_world("lab-slow").await;
    let mut c = Raw::connect(&l.w, "slow-client").await;
    let began = Instant::now();
    c.try_call(ApiKey::Fetch, 12, &fetch_req("orders"))
        .await
        .unwrap();
    let plain = began.elapsed();
    let id = make(
        &l,
        LabKind::Slow,
        &[
            ("api", "fetch"),
            ("topic", "orders"),
            ("delay", "300"),
            ("share", "every"),
            ("ends", "5"),
        ],
    )
    .await;
    let began = Instant::now();
    let a = c
        .try_call(ApiKey::Fetch, 12, &fetch_req("orders"))
        .await
        .unwrap();
    let slow = began.elapsed();
    let r: FetchResponse = body(ApiKey::Fetch, 12, a);
    assert_eq!(r.responses[0].partitions[0].error_code, 0);
    assert!(
        slow >= Duration::from_millis(300),
        "{slow:?} vs plain {plain:?}"
    );
    let hits = wire_hits(&l, id);
    assert_eq!(hits.len(), 1);
    let lab = hits[0].lab.clone().unwrap();
    assert_eq!((lab.rule, lab.what), (id, LabDid::Delayed(300)));
    assert_eq!(hits[0].took_sub.as_deref(), Some("includes +300 ms"));
    let v = glab::rule(&l.lab, Some(&l.rec), id).unwrap();
    assert_eq!((v.rule.hits, v.rule.matched), (1, 1));
    assert!(
        v.rule
            .sentence
            .starts_with("+300 ms on every Fetch request to orders"),
        "{}",
        v.rule.sentence
    );
    assert!(glab::remove(&l.lab, id));
    let began = Instant::now();
    c.try_call(ApiKey::Fetch, 12, &fetch_req("orders"))
        .await
        .unwrap();
    assert!(began.elapsed() < Duration::from_millis(250));
    assert_eq!(wire_hits(&l, id).len(), 1, "the later fetch is not tagged");
    let home = glab::home(&l.lab, &l.w.backend).await;
    assert!(home.active.is_empty() && home.ended.len() == 1 && home.ended[0].ended.is_some());
    l.w.stop().await;
}

/// LAB-15: a FAIL rule answers Produce with its code and the partition's offset unset, and the log is as it was.
#[tokio::test]
async fn a_fail_rule_answers_produce_with_its_code_and_writes_nothing() {
    let l = lab_world("lab-fail").await;
    make(
        &l,
        LabKind::Fail,
        &[
            ("api", "produce"),
            ("topic", "orders"),
            ("share", "every"),
            ("code", "7"),
            ("ends", "5"),
        ],
    )
    .await;
    let mut c = Raw::connect(&l.w, "fail-client").await;
    let a = c
        .try_call(ApiKey::Produce, 9, &produce_req("orders"))
        .await
        .unwrap();
    let r: ProduceResponse = body(ApiKey::Produce, 9, a);
    let p = &r.responses[0].partition_responses[0];
    assert_eq!((p.error_code, p.base_offset), (7, -1));
    assert_eq!(l.w.offset("orders", 0, -1).await, 0, "nothing was appended");
    let hits = wire_hits(&l, 1);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].code, 7);
    assert_eq!(hits[0].lab.as_ref().unwrap().what, LabDid::Failed);
    l.w.stop().await;
}

/// LAB-16: Drop once closes what the app has open at its next request and leaves the rest; a new connection is served.
#[tokio::test]
async fn drop_once_closes_the_connections_that_were_open_and_not_the_next_one() {
    let l = lab_world("lab-drop").await;
    let mut old = Raw::connect(&l.w, "catalog").await;
    let mut other = Raw::connect(&l.w, "payments").await;
    old.try_call(ApiKey::Metadata, 12, &MetadataRequest::default())
        .await
        .unwrap();
    other
        .try_call(ApiKey::Metadata, 12, &MetadataRequest::default())
        .await
        .unwrap();
    let form = glab::drop_form(Some(&l.rec), "catalog", "once");
    assert_eq!(
        form.choices
            .iter()
            .map(|c| c.value.as_str())
            .collect::<Vec<_>>(),
        ["catalog", "payments"]
    );
    assert_eq!(
        glab::drop_connections(&l.lab, Some(&l.rec), "catalog", "once"),
        LabOnceDone::Done
    );
    assert!(
        old.try_call(ApiKey::Metadata, 12, &MetadataRequest::default())
            .await
            .is_none()
    );
    assert!(
        other
            .try_call(ApiKey::Metadata, 12, &MetadataRequest::default())
            .await
            .is_some()
    );
    let mut fresh = Raw::connect(&l.w, "catalog").await;
    assert!(
        fresh
            .try_call(ApiKey::Metadata, 12, &MetadataRequest::default())
            .await
            .is_some()
    );
    // An app with nothing open is told so.
    let LabOnceDone::Invalid(f) = glab::drop_connections(&l.lab, Some(&l.rec), "nobody", "once")
    else {
        panic!()
    };
    assert!(f.error.unwrap().contains("no connection open"));
    l.w.stop().await;
}

fn join(group: &str, member: &str) -> JoinGroupRequest {
    let mut r = JoinGroupRequest::default();
    r.group_id = GroupId(s(group));
    r.member_id = s(member);
    r.session_timeout_ms = 30_000;
    r.rebalance_timeout_ms = 30_000;
    r.protocol_type = s("consumer");
    let mut p = JoinGroupRequestProtocol::default();
    p.name = s("range");
    r.protocols = vec![p];
    r
}

async fn classic_member(w: &World, group: &str) -> String {
    let Response::JoinGroup(j) = w.ask(3, Request::JoinGroup(join(group, ""))).await else {
        panic!()
    };
    assert_eq!(j.error_code, 0, "{j:?}");
    let mut sync = SyncGroupRequest::default();
    sync.group_id = GroupId(s(group));
    sync.generation_id = j.generation_id;
    sync.member_id = j.member_id.clone();
    let Response::SyncGroup(a) = w.ask(1, Request::SyncGroup(sync)).await else {
        panic!()
    };
    assert_eq!(a.error_code, 0);
    j.member_id.to_string()
}

async fn heartbeat(w: &World, group: &str, member: &str, generation: i32) -> i16 {
    let mut h = HeartbeatRequest::default();
    h.group_id = GroupId(s(group));
    h.member_id = s(member);
    h.generation_id = generation;
    let Response::Heartbeat(r) = w.ask(1, Request::Heartbeat(h)).await else {
        panic!()
    };
    r.error_code
}

/// LAB-17 and LAB-18: an expired member is gone through the group's own session path (the story says session expired, the next
/// heartbeat is told it is unknown); a forced rebalance makes the members rejoin, and says it was forced.
#[tokio::test]
async fn an_expired_member_and_a_forced_rebalance_go_through_the_groups_own_machine() {
    let l = lab_world("lab-groups").await;
    let a = classic_member(&l.w, "fulfilment").await;
    assert_eq!(heartbeat(&l.w, "fulfilment", &a, 1).await, 0);
    let form = glab::expire_form(&l.w.backend, "").await;
    assert_eq!(form.choices.len(), 1);
    assert!(form.choices[0].label.contains(&a));
    // Rebalance first: the member is told to rejoin at its next heartbeat.
    assert_eq!(
        glab::rebalance(&l.lab, &l.w.backend, "fulfilment").await,
        LabOnceDone::Done
    );
    assert_eq!(heartbeat(&l.w, "fulfilment", &a, 1).await, 27);
    let LabOnceDone::Invalid(again) = glab::rebalance(&l.lab, &l.w.backend, "fulfilment").await
    else {
        panic!("a rebalance already running is said so")
    };
    assert!(again.refused.unwrap().contains("rebalancing already"));
    let events = l.w.backend.group_events("fulfilment").await.unwrap();
    let causes: Vec<_> = events.events.iter().map(|e| format!("{e:?}")).collect();
    assert!(
        causes.iter().any(|c| c.contains("cause: Some(Forced)")),
        "{causes:?}"
    );
    // Another group, running: its member is expired, and the story says the session ran out.
    let b = classic_member(&l.w, "payments").await;
    let target = kafka_console::lab::act::member_value("payments", &b);
    assert_eq!(
        glab::expire(&l.lab, &l.w.backend, &target).await,
        LabOnceDone::Done
    );
    assert_eq!(
        heartbeat(&l.w, "payments", &b, 1).await,
        25,
        "UNKNOWN_MEMBER_ID"
    );
    let events = l.w.backend.group_events("payments").await.unwrap();
    let causes: Vec<_> = events.events.iter().map(|e| format!("{e:?}")).collect();
    assert!(
        causes
            .iter()
            .any(|c| c.contains("cause: Some(SessionExpired)")),
        "{causes:?}"
    );
    // No such group or member.
    let LabOnceDone::Invalid(f) = glab::rebalance(&l.lab, &l.w.backend, "nope").await else {
        panic!()
    };
    assert!(f.refused.unwrap().contains("no such group"));
    let home = glab::home(&l.lab, &l.w.backend).await;
    assert!(home.once.iter().filter(|o| o.ok).count() >= 2 && home.once.iter().any(|o| !o.ok));
    l.w.stop().await;
}

/// LAB-19: a group of the new protocol moves to the next epoch when a rebalance is forced.
#[tokio::test]
async fn a_forced_rebalance_moves_a_consumer_group_to_the_next_epoch() {
    let l = lab_world("lab-848").await;
    let mut hb = ConsumerGroupHeartbeatRequest::default();
    hb.group_id = GroupId(s("payments-workers"));
    hb.member_id = s("m-1");
    hb.member_epoch = 0;
    hb.rebalance_timeout_ms = 30_000;
    hb.subscribed_topic_names = Some(vec![TopicName(s("orders"))]);
    hb.topic_partitions = Some(vec![]);
    let Response::ConsumerGroupHeartbeat(r) = l.w.ask(0, Request::ConsumerGroupHeartbeat(hb)).await
    else {
        panic!()
    };
    assert_eq!(r.error_code, 0, "{r:?}");
    let before =
        l.w.backend
            .group_detail("payments-workers")
            .await
            .unwrap()
            .generation;
    assert_eq!(
        glab::rebalance(&l.lab, &l.w.backend, "payments-workers").await,
        LabOnceDone::Done
    );
    let after =
        l.w.backend
            .group_detail("payments-workers")
            .await
            .unwrap()
            .generation;
    assert_eq!(after, before + 1);
    l.w.stop().await;
}

/// LAB-20: Compact now runs the cleaner's round on a due partition, shows the planner's verdict first, and says no when nothing is due
/// or the topic does not compact.
#[tokio::test]
async fn compact_now_runs_the_real_round_and_refuses_with_a_sentence_otherwise() {
    let l = lab_world("lab-compact").await;
    l.w.create_topic(
        "inventory.levels",
        1,
        &[("cleanup.policy", "compact"), ("segment.bytes", "1024")],
    )
    .await;
    let none = glab::compact_form(&l.w.backend, true, "inventory.levels", 0)
        .await
        .unwrap();
    assert!(none.due.is_none() && none.blocked.is_some());
    let err = glab::compact(&l.lab, &l.w.backend, "inventory.levels", 0)
        .await
        .unwrap_err();
    let why = err.refused.unwrap();
    assert!(why.contains("nothing"), "{why}");
    l.w.fill("inventory.levels", 120, 5).await;
    let due = glab::compact_form(&l.w.backend, true, "inventory.levels", 0)
        .await
        .unwrap();
    assert!(
        due.due.as_deref().unwrap().contains("dirty against"),
        "{due:?}"
    );
    let _ = keyed(&[]);
    glab::compact(&l.lab, &l.w.backend, "inventory.levels", 0)
        .await
        .unwrap();
    let after = glab::compact_form(&l.w.backend, true, "inventory.levels", 0)
        .await
        .unwrap();
    assert!(after.due.is_none(), "the round left nothing due: {after:?}");
    let home = glab::home(&l.lab, &l.w.backend).await;
    assert_eq!(home.compact_target.as_deref(), Some("inventory.levels"));
    assert!(
        home.once
            .iter()
            .any(|o| o.ok && o.sentence.contains("Ran the cleaner"))
    );
    // A topic that does not compact.
    let plain = glab::compact_form(&l.w.backend, true, "orders", 0)
        .await
        .unwrap();
    assert!(plain.blocked.unwrap().contains("does not compact"));
    assert!(
        glab::compact_form(&l.w.backend, true, "missing", 0)
            .await
            .is_none()
    );
    l.w.stop().await;
}

/// LAB-21: the console may only do what the broker lets it: a caller without the right is refused, as a client would be.
#[tokio::test]
async fn a_caller_without_the_right_is_refused_by_the_broker() {
    let l = lab_world("lab-acl").await;
    let a = classic_member(&l.w, "g").await;
    let nobody =
        l.w.backend
            .acting_as(Caller::new(Principal::user("nobody"), None));
    // The default allows everyone here, so deny by an ACL-free broker with default deny is the other world; with default allow the
    // member is expired for any caller, and a missing group is still said.
    assert_eq!(nobody.lab_expire("g", &a).await, LabOutcome::Done);
    assert_eq!(
        nobody.lab_rebalance("absent").await,
        LabOutcome::NoSuchGroup
    );
    l.w.stop().await;
    let table = FaultTable::new();
    let t = Arc::clone(&table);
    let w = world("lab-acl-deny", move |bc, sc| {
        bc.acl_default_allow = false;
        sc.fault = Some(t);
    })
    .await;
    let nobody = w
        .backend
        .acting_as(Caller::new(Principal::user("nobody"), None));
    assert_eq!(nobody.lab_expire("g", "m").await, LabOutcome::Denied);
    assert_eq!(nobody.lab_rebalance("g").await, LabOutcome::Denied);
    assert_eq!(
        nobody.lab_clean("orders", 0).await,
        kafka_broker::CleanOutcome::Denied
    );
    assert_eq!(
        w.backend.lab_rebalance("g").await,
        LabOutcome::NoSuchGroup,
        "the console is a super user"
    );
    w.stop().await;
}

/// LAB-22: names that are hostile are text in every field a view-model carries, and a rule that cannot be made says why.
#[tokio::test]
async fn hostile_topic_and_client_patterns_are_kept_as_text_and_bad_fields_are_said() {
    let l = lab_world("lab-hostile").await;
    let hostile = "<img src=x onerror=alert(1)>";
    let id = make(
        &l,
        LabKind::Slow,
        &[
            ("api", "produce"),
            ("topic", hostile),
            ("client", "<script>"),
            ("delay", "10"),
            ("share", "every"),
            ("ends", "1"),
        ],
    )
    .await;
    let v = glab::rule(&l.lab, Some(&l.rec), id).unwrap();
    assert!(v.rule.name.contains(hostile) && v.rule.sentence.contains(hostile));
    let LabMade::Invalid(f) = glab::create(
        &l.lab,
        &l.w.backend,
        Some(&l.rec),
        LabKind::Slow,
        fields(&[
            ("api", "delete-everything"),
            ("delay", "0"),
            ("share", "x"),
            ("ends", "99"),
        ]),
    )
    .await
    else {
        panic!()
    };
    let bad: Vec<&str> = f.errors.iter().map(|(k, _)| k.as_str()).collect();
    for want in ["api", "delay", "share", "ends"] {
        assert!(bad.contains(&want), "{bad:?}");
    }
    // The 51st rule is refused with the limit in words.
    for n in 0..49 {
        make(
            &l,
            LabKind::Slow,
            &[
                ("api", "produce"),
                ("topic", &format!("t{n}")),
                ("delay", "10"),
                ("share", "every"),
                ("ends", "1"),
            ],
        )
        .await;
    }
    let LabMade::Invalid(f) = glab::create(
        &l.lab,
        &l.w.backend,
        Some(&l.rec),
        LabKind::Slow,
        fields(&[
            ("api", "fetch"),
            ("topic", "last"),
            ("delay", "10"),
            ("share", "every"),
            ("ends", "1"),
        ]),
    )
    .await
    else {
        panic!()
    };
    assert!(f.refused.unwrap().contains("50 rules"));
    l.w.stop().await;
}
