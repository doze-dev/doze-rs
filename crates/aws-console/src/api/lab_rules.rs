//! The Lab's fault rules: what a person makes in the console to bend the calls their apps send to doze-aws.
//!
//! - `GET  /api/v1/lab/rules`: the rules in force, the ones that ended, and the bounds.
//! - `POST /api/v1/lab/rules/check`: the sentence a rule would be and everything wrong with it, in words; nothing is made.
//! - `POST /api/v1/lab/rules`: makes the rule, or says why not.
//! - `GET  /api/v1/lab/rules/{id}`: one rule, in force or over, and the calls on Live it touched.
//! - `POST /api/v1/lab/rules/{id}/switch`, `POST /api/v1/lab/rules/{id}/remove`: switch it on or off; end it.
//! - `POST /api/v1/lab/stop-all`, `POST /api/v1/lab/history/clear`: end every rule; forget the ones that ended.
//! - `GET  /api/v1/lab/guide`: the words (`crate::lab::teach`).
//!
//! Every write is behind the same-origin proof like every write of this API, and the table is in memory: no file, flag or
//! environment makes a rule. With the Lab off (`--lab=false`) the lists are empty, nothing can be made, and the answer says why.

use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};

use super::lab_build::{build, check as check_rule, limits, rule_view};
use super::{error, json};
use crate::lab::LabTable;
use crate::model::{
    LabCleared, LabMade, LabProblem, LabRuleBody, LabRuleView, LabRules, LabStopped, LabSwitchBody,
};
use crate::state::Shared;

/// The calls of a rule's page.
const CALLS_SHOWN: usize = 100;

/// The routes of the Lab's rules.
pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/v1/lab/rules", get(rules).post(create))
        .route("/api/v1/lab/rules/check", post(check))
        .route("/api/v1/lab/rules/{id}", get(one))
        .route("/api/v1/lab/rules/{id}/switch", post(switch))
        .route("/api/v1/lab/rules/{id}/remove", post(remove))
        .route("/api/v1/lab/stop-all", post(stop_all))
        .route("/api/v1/lab/history/clear", post(clear_history))
        .route("/api/v1/lab/guide", get(guide))
}

/// The table, or the answer that says the Lab is off.
fn table(s: &Shared) -> Result<&Arc<LabTable>, Box<Response>> {
    s.facts.lab.table.as_ref().ok_or_else(|| {
        Box::new(error(
            StatusCode::CONFLICT,
            "LAB_OFF",
            s.facts
                .lab
                .off_why
                .as_deref()
                .unwrap_or("The Lab is off: doze-aws was started with --lab=false."),
        ))
    })
}

/// A rule's number as the address wrote it.
fn number(id: &str) -> Result<u32, Box<Response>> {
    id.parse().map_err(|_| {
        Box::new(error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            &format!("{id} is not the number of a rule."),
        ))
    })
}

fn no_rule(id: u32) -> Response {
    error(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        &format!("No rule is numbered {id}: it may have been forgotten, or doze-aws restarted."),
    )
}

fn body_of(bytes: &Bytes) -> Result<LabRuleBody, Box<Response>> {
    serde_json::from_slice(bytes).map_err(|e| {
        Box::new(error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            &format!("The rule is a JSON object with an effect and ends_in_secs: {e}."),
        ))
    })
}

/// `GET /api/v1/lab/rules`.
pub async fn rules(State(s): State<Shared>) -> Response {
    let Some(table) = s.facts.lab.table.as_ref() else {
        return json(
            StatusCode::OK,
            &LabRules {
                enabled: false,
                off_why: s.facts.lab.off_why.clone(),
                now_ms: crate::ring::now_ms().try_into().unwrap_or(i64::MAX),
                in_force: Vec::new(),
                ended: Vec::new(),
                limits: limits(),
            },
        );
    };
    let facts = &s.facts;
    json(
        StatusCode::OK,
        &LabRules {
            enabled: true,
            off_why: None,
            now_ms: table.now_ms(),
            in_force: table
                .rules()
                .iter()
                .map(|r| rule_view(facts, r, None))
                .collect(),
            ended: table
                .ended()
                .iter()
                .map(|e| rule_view(facts, &e.rule, Some((e.at_ms, e.why))))
                .collect(),
            limits: limits(),
        },
    )
}

/// `POST /api/v1/lab/rules/check`.
pub async fn check(State(s): State<Shared>, bytes: Bytes) -> Response {
    let body = match body_of(&bytes) {
        Ok(b) => b,
        Err(res) => return *res,
    };
    let Some(table) = s.facts.lab.table.as_ref() else {
        let mut said = check_rule(&s.facts, &body, |_| true);
        said.ok = false;
        said.problems.push(LabProblem {
            field: "rules".to_owned(),
            text: s
                .facts
                .lab
                .off_why
                .clone()
                .unwrap_or_else(|| "The Lab is off.".to_owned()),
        });
        return json(StatusCode::OK, &said);
    };
    json(
        StatusCode::OK,
        &check_rule(&s.facts, &body, |spec| table.has_room_for(spec)),
    )
}

/// `POST /api/v1/lab/rules`: the rule that was made, or the check that says why not (it is also what the form is told).
pub async fn create(State(s): State<Shared>, bytes: Bytes) -> Response {
    let body = match body_of(&bytes) {
        Ok(b) => b,
        Err(res) => return *res,
    };
    let table = match table(&s) {
        Ok(t) => t,
        Err(res) => return *res,
    };
    let (spec, problems) = build(&s.facts, &body);
    let made = if problems.is_empty() {
        table.add(spec).ok().and_then(|id| table.find(id))
    } else {
        None
    };
    match made {
        Some(found) => json(
            StatusCode::OK,
            &LabMade::Made {
                rule: Box::new(rule_view(&s.facts, &found.rule, None)),
            },
        ),
        None => json(
            StatusCode::OK,
            &LabMade::Refused {
                check: check_rule(&s.facts, &body, |spec| table.has_room_for(spec)),
            },
        ),
    }
}

/// `GET /api/v1/lab/rules/{id}`.
pub async fn one(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let table = match table(&s) {
        Ok(t) => t,
        Err(res) => return *res,
    };
    let id = match number(&id) {
        Ok(id) => id,
        Err(res) => return *res,
    };
    let Some(found) = table.find(id) else {
        return no_rule(id);
    };
    let (calls, more) = s.recorder.touched(Some(id), CALLS_SHOWN);
    json(
        StatusCode::OK,
        &LabRuleView {
            rule: rule_view(&s.facts, &found.rule, found.over),
            now_ms: table.now_ms(),
            calls: calls
                .iter()
                .map(|c| crate::model::CallRecord::new(c, &s.facts))
                .collect(),
            more,
        },
    )
}

/// `POST /api/v1/lab/rules/{id}/switch`.
pub async fn switch(State(s): State<Shared>, Path(id): Path<String>, bytes: Bytes) -> Response {
    let table = match table(&s) {
        Ok(t) => t,
        Err(res) => return *res,
    };
    let id = match number(&id) {
        Ok(id) => id,
        Err(res) => return *res,
    };
    let asked: LabSwitchBody = if bytes.iter().all(u8::is_ascii_whitespace) {
        LabSwitchBody::default()
    } else {
        match serde_json::from_slice(&bytes) {
            Ok(b) => b,
            Err(e) => {
                return error(
                    StatusCode::BAD_REQUEST,
                    "BAD_REQUEST",
                    &format!("The body is {{\"enabled\": true or false}}, or nothing: {e}."),
                );
            }
        }
    };
    let Some(found) = table.find(id).filter(|f| f.over.is_none()) else {
        return match table.find(id) {
            Some(_) => error(
                StatusCode::CONFLICT,
                "OVER",
                &format!("Rule {id} is over: it cannot be switched. Make it again to use it."),
            ),
            None => no_rule(id),
        };
    };
    let on = asked.enabled.unwrap_or(!found.rule.enabled);
    table.set_enabled(id, on);
    match table.find(id) {
        Some(now) => json(StatusCode::OK, &rule_view(&s.facts, &now.rule, now.over)),
        None => no_rule(id),
    }
}

/// `POST /api/v1/lab/rules/{id}/remove`.
pub async fn remove(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let table = match table(&s) {
        Ok(t) => t,
        Err(res) => return *res,
    };
    let id = match number(&id) {
        Ok(id) => id,
        Err(res) => return *res,
    };
    if !table.remove(id) {
        return match table.find(id) {
            Some(_) => error(
                StatusCode::CONFLICT,
                "OVER",
                &format!("Rule {id} is over already."),
            ),
            None => no_rule(id),
        };
    }
    match table.find(id) {
        Some(now) => json(StatusCode::OK, &rule_view(&s.facts, &now.rule, now.over)),
        None => no_rule(id),
    }
}

/// `POST /api/v1/lab/stop-all`.
pub async fn stop_all(State(s): State<Shared>) -> Response {
    let table = match table(&s) {
        Ok(t) => t,
        Err(res) => return *res,
    };
    let stopped = u32::try_from(table.stop_all()).unwrap_or(u32::MAX);
    json(StatusCode::OK, &LabStopped { stopped })
}

/// `POST /api/v1/lab/history/clear`.
pub async fn clear_history(State(s): State<Shared>) -> Response {
    let table = match table(&s) {
        Ok(t) => t,
        Err(res) => return *res,
    };
    let cleared = u32::try_from(table.clear_ended()).unwrap_or(u32::MAX);
    json(StatusCode::OK, &LabCleared { cleared })
}

/// `GET /api/v1/lab/guide`. The words do not depend on the Lab being on: a person can read what it would do.
pub async fn guide() -> Response {
    json(StatusCode::OK, &crate::lab::teach::guide())
}
