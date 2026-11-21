//! Settings: this doze-aws's own facts and the one thing it can change from here.
//!
//! - `GET /api/v1/settings`: every setting it was started with and where each came from, the data directory and what each
//!   service keeps there (`doze-aws data`'s report, read when asked), the Live record's size.
//! - `POST /api/v1/settings/reset` `{"dry_run": bool}`: removes everything every service keeps, as `doze-aws data reset` does
//!   (the same engine); with `dry_run`, says what it would remove and removes nothing. Behind the same-origin proof.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::Value;

use super::calls::fill;
use super::{at, error, json};
use crate::model::{DataUse, FlagRow, ResetDone, ResetRow, SettingsView};
use crate::ring::{MOST_TEXT, RECORD_BYTES};
use crate::state::Shared;

/// `GET /api/v1/settings`.
pub async fn settings(State(s): State<Shared>) -> Response {
    let f = &s.facts;
    let report = match &f.data {
        Some(data) => data.report().await,
        None => Err("This console was started without the data engine.".to_owned()),
    };
    let (data_bytes, mut data, data_error) = match report {
        Ok(r) => (
            at(&r, "bytes").as_u64(),
            at(&r, "services")
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| DataUse {
                    name: at(p, "name").as_str().unwrap_or_default().to_owned(),
                    title: at(p, "title").as_str().unwrap_or_default().to_owned(),
                    bytes: at(p, "bytes").as_u64().unwrap_or(0),
                })
                .collect::<Vec<_>>(),
            None,
        ),
        Err(why) => (None, Vec::new(), Some(why)),
    };
    data.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.name.cmp(&b.name)));
    let most = s.recorder.capacity() * (RECORD_BYTES + MOST_TEXT);
    json(
        StatusCode::OK,
        &SettingsView {
            version: f.version.clone(),
            flags: f
                .flags
                .iter()
                .map(|g| FlagRow {
                    name: g.name.clone(),
                    value: g.value.clone(),
                    source: g.source.clone(),
                    default: g.default,
                })
                .collect(),
            data_dir: f.data_dir.clone(),
            data_bytes,
            data,
            data_error,
            ring: fill(&s),
            ring_most_bytes: u64::try_from(most).unwrap_or(u64::MAX),
            reset: f.data.is_some(),
        },
    )
}

/// The rows of a reset's answer.
fn rows(answer: &Value) -> Vec<ResetRow> {
    at(answer, "services")
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| ResetRow {
            service: at(r, "service").as_str().unwrap_or_default().to_owned(),
            items: at(r, "items").as_u64().unwrap_or(0),
            bytes: at(r, "bytes").as_u64().unwrap_or(0),
            freed: at(r, "freed").as_u64().unwrap_or(0),
        })
        .collect()
}

/// `POST /api/v1/settings/reset`.
pub async fn reset(State(s): State<Shared>, body: Bytes) -> Response {
    let Some(dry_run) = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|v| at(&v, "dry_run").as_bool())
    else {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "A reset says whether it is a dry run: {\"dry_run\": true} or {\"dry_run\": false}.",
        );
    };
    let Some(data) = &s.facts.data else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "NO_DATA",
            "This console was started without the data engine: run doze-aws data reset.",
        );
    };
    match data.reset(dry_run).await {
        Ok(answer) => json(
            StatusCode::OK,
            &ResetDone {
                dry_run,
                services: rows(&answer),
            },
        ),
        Err(why) => error(StatusCode::CONFLICT, "RESET_FAILED", &why),
    }
}

#[cfg(test)]
mod tests {
    use super::rows;
    use serde_json::json;

    #[test]
    fn a_reset_answer_is_read_row_by_row() {
        let got = rows(
            &json!({"services": [{"service": "sqs", "items": 2, "bytes": 10, "freed": 4}, {}]}),
        );
        assert_eq!(got.len(), 2);
        assert_eq!(
            (got[0].service.as_str(), got[0].items, got[0].freed),
            ("sqs", 2, 4)
        );
        assert_eq!(got[1].service, "");
    }
}
