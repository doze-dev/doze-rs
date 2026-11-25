//! The Lab's forced events: rehearsals that are not rules (`crate::lab_once`).
//!
//! - `GET  /api/v1/lab/once`: the kinds, and what was forced lately.
//! - `GET  /api/v1/lab/once/<kind>/form[?region=&choice=&look=yes]`: what there is to choose in the region, and what forcing it does.
//! - `POST /api/v1/lab/once/<kind>` (`{"choice": ..., "region"?: ..., "time_wake"?: bool}`): carries it out; what came of it.
//! - `POST /api/v1/lab/once/history/clear`: forgets the history.
//! - `GET  /api/v1/lab/once/guide`: the lesson of each kind.
//!
//! Behind the same-origin proof like every write. A forced event is a call the console makes as the account's root on the bus: what it
//! may do is what the person running doze-aws may do.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::Value;

use super::resources::decode;
use super::{error, json, param};
use crate::lab_once::kinds::{self, KINDS};
use crate::lab_once::{act, form, teach};
use crate::model::{LabOnceIndex, LabOnceKind};
use crate::state::Shared;

fn unknown(id: &str) -> Response {
    error(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        &format!("There is no forced event called {id}."),
    )
}

/// The region a request is for: the one it names (which must be one of AWS's), else the console's (`X-Console-Region`, else `--region`).
#[allow(clippy::result_large_err)]
fn region_for(s: &Shared, named: Option<&str>) -> Result<String, Response> {
    match named.filter(|r| !r.is_empty()) {
        Some(r) if crate::regions::is_region(r) => Ok(r.to_owned()),
        Some(_) => Err(error(
            StatusCode::BAD_REQUEST,
            "UNKNOWN_REGION",
            "That is not a region AWS has.",
        )),
        None => Ok(crate::regions::current(&s.facts.region)),
    }
}

/// `GET /api/v1/lab/once`.
pub async fn index(State(s): State<Shared>) -> Response {
    let kinds: Vec<LabOnceKind> = KINDS.iter().map(|k| k.shown(&s)).collect();
    json(
        StatusCode::OK,
        &LabOnceIndex {
            kinds,
            history: s.once.recent(),
        },
    )
}

/// `GET /api/v1/lab/once/<kind>/form`.
pub async fn form(
    State(s): State<Shared>,
    Path(id): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let Some(def) = kinds::find(&id) else {
        return unknown(&id);
    };
    let region = param(query.as_deref(), "region").map(decode);
    let region = match region_for(&s, region.as_deref()) {
        Ok(r) => r,
        Err(res) => return res,
    };
    let choice = param(query.as_deref(), "choice").map(decode);
    let look = param(query.as_deref(), "look") == Some("yes");
    json(
        StatusCode::OK,
        &form::build(&s, def, &region, choice.as_deref(), look).await,
    )
}

/// `POST /api/v1/lab/once/<kind>`.
pub async fn force(State(s): State<Shared>, Path(id): Path<String>, body: Bytes) -> Response {
    let Some(def) = kinds::find(&id) else {
        return unknown(&id);
    };
    let Ok(body @ Value::Object(_)) = serde_json::from_slice::<Value>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "The request is a JSON object.",
        );
    };
    let region = match region_for(&s, body.get("region").and_then(Value::as_str)) {
        Ok(r) => r,
        Err(res) => return res,
    };
    match act::force(&s, def, &region, &body).await {
        Ok(done) => json(StatusCode::OK, &done),
        Err(words) => error(StatusCode::BAD_REQUEST, "BAD_REQUEST", &words),
    }
}

/// `POST /api/v1/lab/once/history/clear`.
pub async fn clear(State(s): State<Shared>) -> Response {
    s.once.clear();
    json(
        StatusCode::OK,
        &LabOnceIndex {
            kinds: KINDS.iter().map(|k| k.shown(&s)).collect(),
            history: Vec::new(),
        },
    )
}

/// `GET /api/v1/lab/once/guide`.
pub async fn guide() -> Response {
    json(StatusCode::OK, &teach::guide())
}

/// The TypeScript types of this part of the API, written beside the others by `api::types`.
#[cfg(test)]
pub(super) fn register(cfg: &ts_rs::Config, out: &mut std::collections::BTreeMap<String, String>) {
    use crate::model::{
        LabOnce, LabOnceChoice, LabOnceDone, LabOnceForm, LabOnceGuide, LabOnceOption,
        LabOnceSetting, LabOnceTeach,
    };
    use ts_rs::TS;
    fn add<T: TS + 'static + ?Sized>(
        cfg: &ts_rs::Config,
        out: &mut std::collections::BTreeMap<String, String>,
    ) {
        let name = T::name(cfg);
        let text = T::export_to_string(cfg).unwrap_or_else(|e| panic!("{name}: {e}"));
        out.insert(format!("{name}.ts"), text);
    }
    add::<LabOnceIndex>(cfg, out);
    add::<LabOnceKind>(cfg, out);
    add::<LabOnce>(cfg, out);
    add::<LabOnceForm>(cfg, out);
    add::<LabOnceChoice>(cfg, out);
    add::<LabOnceOption>(cfg, out);
    add::<LabOnceDone>(cfg, out);
    add::<LabOnceGuide>(cfg, out);
    add::<LabOnceTeach>(cfg, out);
    add::<LabOnceSetting>(cfg, out);
}
