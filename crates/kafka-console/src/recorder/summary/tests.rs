//! A golden file per API: what `Summarize` keeps of a representative request and its answer. A new API without a case here fails
//! `every_served_api_has_a_golden_file`; one without a summary does not compile. `BLESS=1 cargo test` writes the files.

use kafka_wire::api::{API_TABLE, Request, Response};
use kafka_wire::messages::*;

mod cases;
mod cases_more;
mod cases_share;
mod cases_streams;

use super::*;
use crate::recorder::facts::Facts;
use cases::cases;

fn facts_of(req: &Request, resp: &Response) -> Facts {
    let mut f = Facts::default();
    of_request(req, &mut f);
    of_response(resp, &mut f);
    f
}

fn golden_path(key: ApiKey) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/recorder/tests/golden")
        .join(format!("{key:?}.txt"))
}

#[test]
fn every_served_api_has_a_golden_file() {
    let have: Vec<ApiKey> = cases().iter().map(|c| c.0).collect();
    for row in API_TABLE {
        assert!(
            have.contains(&row.key),
            "{:?} has no case in summary/tests.rs",
            row.key
        );
    }
    assert_eq!(
        have.len(),
        API_TABLE.len(),
        "a case for an API the table does not list"
    );
}

#[test]
fn each_summary_is_what_its_golden_file_says() {
    let bless = std::env::var_os("BLESS").is_some();
    for (key, req, resp) in cases() {
        let text = facts_of(&req, &resp).canon();
        let path = golden_path(key);
        if bless {
            std::fs::write(&path, &text).unwrap();
        }
        let want = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("no golden file for {key:?}: run with BLESS=1"));
        assert_eq!(text, want, "{key:?} (BLESS=1 rewrites the golden files)");
    }
}

#[test]
fn no_planted_secret_or_record_byte_reaches_a_summary() {
    for (key, req, resp) in cases() {
        let text = format!("{:?}", facts_of(&req, &resp));
        assert!(
            !text.contains("PLANTED"),
            "{key:?} kept a planted secret: {text}"
        );
    }
}

#[test]
fn a_summary_of_a_default_request_and_answer_never_panics_and_stays_small() {
    for (key, req, resp) in cases() {
        let f = facts_of(&req, &resp);
        assert!(f.topics.len() <= crate::recorder::facts::TOPICS, "{key:?}");
        assert!(
            f.results.len() <= crate::recorder::facts::RESULTS,
            "{key:?}"
        );
    }
}

/// Produce v13 and TxnOffsetCommit v6 name their topics by id (KIP-516): the summary shows the id, as Fetch's does.
#[test]
fn a_topic_named_by_id_is_shown_by_its_id() {
    let id = kafka_wire::Uuid::from_u128(0xabc);
    let (mut req, mut resp) = cases::produce();
    if let (Request::Produce(q), Response::Produce(a)) = (&mut req, &mut resp) {
        q.topic_data[0].name = cases::topic("");
        q.topic_data[0].topic_id = id;
        a.responses[0].name = cases::topic("");
        a.responses[0].topic_id = id;
    }
    let f = facts_of(&req, &resp);
    assert_eq!(&*f.topics[0].name, id.to_string());
    assert_eq!(&*f.results[0].topic, id.to_string());
    let mut commit = TxnOffsetCommitRequest::default();
    let mut t = txn_offset_commit_request::TxnOffsetCommitRequestTopic::default();
    t.topic_id = id;
    t.partitions = vec![Default::default()];
    commit.topics = vec![t];
    let mut answer = TxnOffsetCommitResponse::default();
    let mut rt = txn_offset_commit_response::TxnOffsetCommitResponseTopic::default();
    rt.topic_id = id;
    rt.partitions = vec![Default::default()];
    answer.topics = vec![rt];
    let f = facts_of(
        &Request::TxnOffsetCommit(commit),
        &Response::TxnOffsetCommit(answer),
    );
    assert_eq!(&*f.topics[0].name, id.to_string());
    assert_eq!(&*f.results[0].topic, id.to_string());
    // OffsetCommit v10 and OffsetFetch v10 (KIP-848) alike.
    use kafka_wire::messages::{offset_commit_request as ocq, offset_commit_response as ocr};
    use kafka_wire::messages::{offset_fetch_request as ofq, offset_fetch_response as ofr};
    let mut commit = kafka_wire::messages::OffsetCommitRequest::default();
    let mut t = ocq::OffsetCommitRequestTopic::default();
    t.topic_id = id;
    t.partitions = vec![Default::default()];
    commit.topics = vec![t];
    let mut answer = kafka_wire::messages::OffsetCommitResponse::default();
    let mut rt = ocr::OffsetCommitResponseTopic::default();
    rt.topic_id = id;
    rt.partitions = vec![Default::default()];
    answer.topics = vec![rt];
    let f = facts_of(
        &Request::OffsetCommit(commit),
        &Response::OffsetCommit(answer),
    );
    assert_eq!(&*f.topics[0].name, id.to_string());
    assert_eq!(&*f.results[0].topic, id.to_string());
    let mut fetch = kafka_wire::messages::OffsetFetchRequest::default();
    let mut g = ofq::OffsetFetchRequestGroup::default();
    let mut ft = ofq::OffsetFetchRequestTopics::default();
    ft.topic_id = id;
    ft.partition_indexes = vec![0];
    g.topics = Some(vec![ft]);
    fetch.groups = vec![g];
    let mut answer = kafka_wire::messages::OffsetFetchResponse::default();
    let mut rg = ofr::OffsetFetchResponseGroup::default();
    let mut rt = ofr::OffsetFetchResponseTopics::default();
    rt.topic_id = id;
    rt.partitions = vec![Default::default()];
    rg.topics = vec![rt];
    answer.groups = vec![rg];
    let f = facts_of(&Request::OffsetFetch(fetch), &Response::OffsetFetch(answer));
    assert_eq!(&*f.topics[0].name, id.to_string());
    assert_eq!(&*f.results[0].topic, id.to_string());
}
