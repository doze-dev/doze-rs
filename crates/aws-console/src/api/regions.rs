//! `GET /api/v1/regions`: the regions of AWS, and which hold something. Reads the data directory's file names; no service is called.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;

use super::json;
use crate::model::{RegionRow, RegionsView};
use crate::regions::{REGIONS, with_data};
use crate::state::Shared;

/// `GET /api/v1/regions`.
pub async fn regions(State(s): State<Shared>) -> Response {
    let held = with_data(std::path::Path::new(&s.facts.data_dir));
    let default = s.facts.region.clone();
    let mut rows: Vec<RegionRow> = REGIONS
        .iter()
        .map(|(code, name)| RegionRow {
            code: (*code).to_owned(),
            name: (*name).to_owned(),
            has_data: held.contains(*code) || *code == default,
        })
        .collect();
    // The default first, then what holds something, then the rest, each by code.
    rows.sort_by(|a, b| {
        (a.code != default, !a.has_data, &a.code).cmp(&(b.code != default, !b.has_data, &b.code))
    });
    json(
        StatusCode::OK,
        &RegionsView {
            default,
            regions: rows,
        },
    )
}
