//! A golden file per API: what `Summarize` keeps of a representative request and its answer. A new API without a case here fails
//! `every_served_api_has_a_golden_file`; one without a summary does not compile. `BLESS=1 cargo test` writes the files.

use kafka_wire::api::{API_TABLE, Request, Response};
use kafka_wire::messages::*;

mod cases;
mod cases_more;

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
