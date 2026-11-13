//! The recorder as the server drives it: begin, end, open, close.

use std::time::Duration;

use kafka_auth::Principal;
use kafka_server::{Answer, Asked, Call, ConnId, Observed, Observer, Opened};
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::*;

use super::*;
use crate::model::{LiveState, WireClass, WireQuery};

fn head(key: ApiKey, version: i16, client: &str) -> RequestHead {
    RequestHead {
        key,
        version,
        correlation_id: 7,
        client_id: Some(StrBytes::from_string(client.to_owned())),
    }
}

/// Plays one request through the observer, as the server does.
pub(super) fn play(
    rec: &Recorder,
    conn: u64,
    client: &str,
    request: &Request,
    answer: Answer<'_>,
    version: i16,
) {
    let who = Principal::anonymous();
    let h = head(request.api_key(), version, client);
    let pending = rec.begin(&Call {
        conn: ConnId(conn),
        principal: &who,
        head: &h,
        asked: Asked::Request(request),
        request_bytes: 100,
    });
    rec.end(
        pending,
        &Observed {
            answer,
            took: Duration::from_micros(1500),
            response_bytes: 40,
            fault: None,
        },
    );
}

fn heartbeat(group: &str, code: i16) -> (Request, Response) {
    let mut q = HeartbeatRequest::default();
    q.group_id = GroupId(StrBytes::from_string(group.to_owned()));
    q.member_id = StrBytes::from_static_str("m-1");
    q.generation_id = 3;
    let mut a = HeartbeatResponse::default();
    a.error_code = code;
    (Request::Heartbeat(q), Response::Heartbeat(a))
}

pub(super) fn produce(topic: &str, records: i32, code: i16) -> (Request, Response) {
    let batch = kafka_wire::batch::build::BatchBuilder::new()
        .records(u32::try_from(records).unwrap())
        .build();
    let mut p = produce_request::PartitionProduceData::default();
    p.records = Some(batch);
    let mut t = produce_request::TopicProduceData::default();
    t.name = TopicName(StrBytes::from_string(topic.to_owned()));
    t.partition_data = vec![p];
    let mut q = ProduceRequest::default();
    q.acks = 1;
    q.topic_data = vec![t];
    let mut pr = produce_response::PartitionProduceResponse::default();
    pr.error_code = code;
    let mut tr = produce_response::TopicProduceResponse::default();
    tr.name = TopicName(StrBytes::from_string(topic.to_owned()));
    tr.partition_responses = vec![pr];
    let mut a = ProduceResponse::default();
    a.responses = vec![tr];
    (Request::Produce(q), Response::Produce(a))
}

pub(super) fn open(rec: &Recorder, conn: u64) {
    rec.opened(&Opened {
        id: ConnId(conn),
        peer: Some("127.0.0.1:50402".parse().unwrap()),
    });
}

#[test]
fn a_request_becomes_a_row_with_its_answer_and_class() {
    let rec = Recorder::new();
    open(&rec, 1);
    let (q, a) = produce("orders", 3, 87);
    play(&rec, 1, "admin-cli", &q, Answer::Response(&a), 9);
    let tape = rec.tape(&WireQuery::default(), now_ms());
    assert_eq!(tape.rows.len(), 1);
    let r = &tape.rows[0];
    assert_eq!(
        (r.api, r.version, r.class, r.code),
        ("Produce", 9, WireClass::Refused, 87)
    );
    assert_eq!(r.code_name, "INVALID_RECORD");
    assert_eq!(
        (r.client.as_str(), r.target.as_str()),
        ("admin-cli", "orders-0")
    );
    assert_eq!(tape.refused_total, 1);
    assert_eq!(tape.refused[0].code_name, "INVALID_RECORD");
    assert_eq!(rec.get(r.id).unwrap().request_bytes, 100);
}

#[test]
fn heartbeats_fold_into_one_row_that_idle_hiding_hides_and_refusals_stand_alone() {
    let rec = Recorder::new();
    open(&rec, 1);
    for _ in 0..200 {
        let (q, a) = heartbeat("fulfilment", 0);
        play(&rec, 1, "fulfilment", &q, Answer::Response(&a), 4);
    }
    let (q, a) = heartbeat("fulfilment", 27);
    play(&rec, 1, "fulfilment", &q, Answer::Response(&a), 4);
    let (q, a) = heartbeat("fulfilment", 25);
    play(&rec, 1, "fulfilment", &q, Answer::Response(&a), 4);
    let (held, _) = rec.held();
    assert_eq!(
        held, 3,
        "200 heartbeats are one row; two different answers are two more"
    );
    let all = rec.tape(
        &WireQuery {
            hide_idle: false,
            ..WireQuery::default()
        },
        now_ms(),
    );
    assert_eq!(all.rows.len(), 3);
    assert_eq!(all.rows.iter().map(|r| r.count).sum::<u32>(), 202);
    assert!(all.rows.iter().any(|r| r.count == 200
        && r.last_ms.is_some()
        && r.took_sub.as_deref() == Some("median of the last 64")));
    assert!(
        all.rows
            .iter()
            .any(|r| r.class == WireClass::Told && r.code == 27)
    );
    let hidden = rec.tape(&WireQuery::default(), now_ms());
    assert_eq!(
        hidden.rows.len(),
        2,
        "the folded heartbeats are the idle ones"
    );
    assert_eq!(hidden.window.requests, 202);
}

#[test]
fn filters_by_client_api_topic_group_and_refusal() {
    let rec = Recorder::new();
    open(&rec, 1);
    open(&rec, 2);
    let (q, a) = produce("orders", 1, 0);
    play(&rec, 1, "orders-app", &q, Answer::Response(&a), 9);
    let (q, a) = produce("inventory", 1, 87);
    play(&rec, 2, "catalog", &q, Answer::Response(&a), 9);
    let (q, a) = heartbeat("fulfilment", 0);
    play(&rec, 2, "catalog", &q, Answer::Response(&a), 4);
    let ask = |q: WireQuery| {
        rec.tape(
            &WireQuery {
                hide_idle: false,
                ..q
            },
            now_ms(),
        )
        .rows
        .len()
    };
    assert_eq!(ask(WireQuery::default()), 3);
    assert_eq!(
        ask(WireQuery {
            client: vec!["catalog".into()],
            ..WireQuery::default()
        }),
        2
    );
    assert_eq!(
        ask(WireQuery {
            api: vec!["Produce".into()],
            ..WireQuery::default()
        }),
        2
    );
    assert_eq!(
        ask(WireQuery {
            topic: vec!["inventory".into()],
            ..WireQuery::default()
        }),
        1
    );
    assert_eq!(
        ask(WireQuery {
            group: "fulfil".into(),
            ..WireQuery::default()
        }),
        1
    );
    assert_eq!(
        ask(WireQuery {
            refused_only: true,
            ..WireQuery::default()
        }),
        1
    );
    assert_eq!(
        ask(WireQuery {
            text: "orders".into(),
            ..WireQuery::default()
        }),
        1
    );
    assert_eq!(
        ask(WireQuery {
            text: "nothing like it".into(),
            ..WireQuery::default()
        }),
        0
    );
}

#[test]
fn the_page_back_and_the_paused_page_are_cut_by_row_id() {
    let rec = Recorder::new();
    open(&rec, 1);
    for i in 0..40 {
        let (q, a) = produce(&format!("t{i}"), 1, 0);
        play(&rec, 1, "app", &q, Answer::Response(&a), 9);
    }
    let first = rec.tape(&WireQuery::default(), now_ms());
    assert_eq!((first.rows.len(), first.matched), (16, 40));
    let older = first.older.unwrap();
    let next = rec.tape(
        &WireQuery {
            before: Some(older),
            ..WireQuery::default()
        },
        now_ms(),
    );
    assert!(next.rows.iter().all(|r| r.id < older));
    assert_eq!(next.rows.len(), 16);
}

#[test]
fn a_900_kib_reason_is_clipped_and_cannot_exceed_the_byte_budget() {
    let rec = Recorder::new();
    open(&rec, 1);
    for _ in 0..3000 {
        let mut q = JoinGroupRequest::default();
        q.group_id = GroupId(StrBytes::from_static_str("g"));
        q.reason = Some(StrBytes::from_string("r".repeat(900 << 10)));
        let mut a = JoinGroupResponse::default();
        a.generation_id = 1;
        play(
            &rec,
            1,
            &"c".repeat(900 << 10),
            &Request::JoinGroup(q),
            Answer::Response(&Response::JoinGroup(a)),
            9,
        );
    }
    let (rows, bytes) = rec.held();
    assert!(bytes <= ring::BUDGET, "{bytes}");
    assert!(
        rec.high_water() <= ring::BUDGET + 20_000,
        "{}",
        rec.high_water()
    );
    assert!(rows > 100, "hundreds of such rows fit: {rows}");
    let tape = rec.tape(&WireQuery::default(), now_ms());
    assert!(tape.rows[0].client.len() <= clip::NAME);
    assert!(tape.rows[0].detail.len() <= clip::TEXT + 20);
}

#[test]
fn a_closed_connection_is_a_row_that_says_why() {
    let rec = Recorder::new();
    open(&rec, 1);
    let who = Principal::anonymous();
    let h = head(ApiKey::Metadata, 12, "stranger");
    let p = rec.begin(&Call {
        conn: ConnId(1),
        principal: &who,
        head: &h,
        asked: Asked::Undecoded,
        request_bytes: 30,
    });
    rec.end(
        p,
        &Observed {
            answer: Answer::Closed("a Metadata request was made out of order in the login"),
            took: Duration::ZERO,
            response_bytes: 0,
            fault: None,
        },
    );
    let tape = rec.tape(&WireQuery::default(), now_ms());
    assert_eq!(tape.rows[0].class, WireClass::Closed);
    assert!(tape.rows[0].detail.contains("out of order"));
}

#[test]
fn clients_are_the_connections_that_are_open_grouped_by_client_id() {
    let rec = Recorder::new();
    open(&rec, 1);
    open(&rec, 2);
    let (q, a) = produce("orders", 1, 0);
    play(&rec, 1, "orders", &q, Answer::Response(&a), 9);
    play(&rec, 2, "orders", &q, Answer::Response(&a), 7);
    let c = rec.clients(now_ms());
    assert_eq!((c.connections, c.rows.len()), (2, 1));
    assert_eq!((c.rows[0].more, c.rows[0].requests), (1, 2));
    assert_eq!(c.rows[0].apis, vec![("Produce", 9)]);
    rec.closed(ConnId(1), "the client closed the connection");
    rec.closed(ConnId(2), "the client closed the connection");
    assert_eq!(rec.clients(now_ms()).connections, 0);
    assert_eq!(
        rec.tape(&WireQuery::default(), now_ms()).rows.len(),
        2,
        "its requests stay on the wire"
    );
}

#[test]
fn a_fetch_names_its_topic_by_id_and_the_recorder_learns_the_name_from_metadata() {
    let rec = Recorder::new();
    open(&rec, 1);
    let id = kafka_wire::Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0);
    let mut t = metadata_response::MetadataResponseTopic::default();
    t.name = Some(TopicName(StrBytes::from_static_str("payments.settled")));
    t.topic_id = id;
    let mut m = MetadataResponse::default();
    m.topics = vec![t];
    play(
        &rec,
        1,
        "app",
        &Request::Metadata(MetadataRequest::default()),
        Answer::Response(&Response::Metadata(m)),
        12,
    );
    let mut ft = fetch_request::FetchTopic::default();
    ft.topic_id = id;
    let mut p = fetch_request::FetchPartition::default();
    p.partition = 1;
    p.fetch_offset = 7402;
    ft.partitions = vec![p];
    let mut q = FetchRequest::default();
    q.topics = vec![ft];
    play(
        &rec,
        1,
        "app",
        &Request::Fetch(q),
        Answer::Response(&Response::Fetch(FetchResponse::default())),
        13,
    );
    let tape = rec.tape(
        &WireQuery {
            hide_idle: false,
            ..WireQuery::default()
        },
        now_ms(),
    );
    let fetch = tape.rows.iter().find(|r| r.api == "Fetch").unwrap();
    assert_eq!(fetch.target, "payments.settled-1");
    assert_eq!(fetch.target_sub.as_deref(), Some("from 7,402"));
    assert_eq!(fetch.detail, "caught up, no records");
}

#[test]
fn the_state_is_quiet_until_a_request_arrives() {
    let rec = Recorder::new();
    assert_eq!(
        rec.tape(&WireQuery::default(), now_ms()).state,
        LiveState::Quiet
    );
    assert_eq!(rec.held(), (0, 0));
    open(&rec, 1);
    let (q, a) = heartbeat("g", 0);
    play(&rec, 1, "c", &q, Answer::Response(&a), 4);
    assert_eq!(
        rec.tape(&WireQuery::default(), now_ms()).state,
        LiveState::Steady
    );
}

#[test]
fn since_returns_what_changed_after_a_cursor_including_a_fold() {
    let rec = Recorder::new();
    open(&rec, 1);
    let (q, a) = heartbeat("g", 0);
    play(&rec, 1, "c", &q, Answer::Response(&a), 4);
    let all = WireQuery {
        hide_idle: false,
        ..WireQuery::default()
    };
    let (cursor, rows) = rec.since(&all, 0);
    assert_eq!(rows.len(), 1);
    assert!(rec.since(&all, cursor).1.is_empty());
    play(&rec, 1, "c", &q, Answer::Response(&a), 4);
    let (next, rows) = rec.since(&all, cursor);
    assert!(next > cursor);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].count, 2);
    assert_eq!(
        rec.tape(&all, now_ms()).cursor,
        next,
        "the page and the tail count from the same version"
    );
}

#[test]
fn the_debug_of_a_recorder_prints_counts_and_nothing_a_client_sent() {
    let rec = Recorder::new();
    open(&rec, 1);
    let (q, a) = produce("PLANTED-TOPIC", 1, 0);
    play(&rec, 1, "PLANTED-CLIENT", &q, Answer::Response(&a), 9);
    let text = format!("{rec:?}");
    assert!(text.contains("rows") && !text.contains("PLANTED"), "{text}");
}

#[test]
fn a_tail_sees_only_the_rows_its_filters_pass() {
    let rec = Recorder::new();
    open(&rec, 1);
    let (p, a) = produce("orders", 1, 0);
    play(&rec, 1, "app", &p, Answer::Response(&a), 9);
    let (p2, a2) = produce("other", 1, 0);
    play(&rec, 1, "app", &p2, Answer::Response(&a2), 9);
    let q = WireQuery {
        topic: vec!["orders".to_owned()],
        ..WireQuery::default()
    };
    let (_, rows) = rec.since(&q, 0);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].topics, vec!["orders".to_owned()]);
}
