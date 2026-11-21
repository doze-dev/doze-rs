//! `GET /api/v1/flows`: the lines of the Live map, who called which service, counted on the request path as calls are answered.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;

use super::json;
use crate::model::{FlowRow, FlowsResponse};
use crate::ring::FLOWS;
use crate::state::Shared;

/// `GET /api/v1/flows`.
pub async fn flows(State(s): State<Shared>) -> Response {
    // The newest number first: a call recorded between the two reads is then in the counts and above `through_seq`, so a reader
    // that adds the stream's calls above it counts it twice at worst, never not at all. Read the other way round it could be lost.
    let (flows, dropped) = s.recorder.flows();
    let through_seq = flows.iter().map(|f| f.last_seq).max().unwrap_or(0);
    json(
        StatusCode::OK,
        &FlowsResponse {
            flows: flows.iter().map(|f| FlowRow::new(f, &s.facts)).collect(),
            capacity: u32::try_from(FLOWS).unwrap_or(u32::MAX),
            dropped,
            through_seq,
        },
    )
}
