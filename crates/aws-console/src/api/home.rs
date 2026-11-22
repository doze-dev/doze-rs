//! Home: is an app connected, who it is, what it called, and what needs a look.
//!
//! - `GET /api/v1/home`: from the Live record alone (no call is made): the apps' callers with the clients their `User-Agent`
//!   named, the services they called, their newest calls, and the signals the record holds (refusals, error bursts, operations
//!   not built).
//! - `GET /api/v1/home/checks`: signals the services hold (a dead-letter queue with messages, an alarm in ALARM, failed
//!   executions, a failed stack), read through the bus as the console, and only from services apps called in this run: Home
//!   never wakes one that sleeps.

use std::collections::{BTreeMap, HashMap};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;

use super::calls::fill;
use super::json;
use crate::iam;
use crate::model::{CallRecord, CallerSeen, HomeView, Outcome, ServiceSeen, Signal, SignalKind};
use crate::resources::bus::CONSOLE;
use crate::ring::Call;
use crate::state::Shared;

mod checks;
pub use checks::checks;

/// Every call the ring holds.
const ALL: usize = usize::MAX;
/// The newest calls Home lists.
const RECENT: usize = 5;
/// The clients a caller lists.
const MOST_SDKS: usize = 3;
/// Errors of one service within [`BURST_MS`] of its newest that make a burst.
const BURST: u64 = 3;
const BURST_MS: u64 = 5 * 60 * 1000;
/// The operations a not-built signal names.
const MOST_NAMED: usize = 3;

/// A call an app made: from the wire, not this console's.
pub(crate) fn from_app(c: &Call) -> bool {
    c.via.is_none() && &*c.caller != CONSOLE
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The signals of the Live record, from the apps' calls (newest first).
fn signals(app: &[&Call], s: &Shared) -> Vec<Signal> {
    let mut out = Vec::new();
    let count = |f: &dyn Fn(&Call) -> bool| app.iter().filter(|c| f(c)).count() as u64;
    let soft = count(&|c| c.would_be_denied);
    if soft > 0 {
        out.push(Signal {
            kind: SignalKind::WouldBeDenied,
            count: soft,
            title: format!("{} be denied", plural(soft, "call would", "calls would")),
            detail: "soft IAM let them through".to_owned(),
            href: Some("/access".to_owned()),
            resource: None,
        });
    }
    let denied = count(&|c| Outcome::of(c.status, &c.code) == Outcome::Denied);
    if denied > 0 {
        out.push(Signal {
            kind: SignalKind::Denied,
            count: denied,
            title: format!("{} denied", plural(denied, "call", "calls")),
            detail: "IAM refused them".to_owned(),
            href: Some("/access".to_owned()),
            resource: None,
        });
    }
    let mut not_built: Vec<&str> = Vec::new();
    let mut unbuilt = 0;
    for c in app
        .iter()
        .filter(|c| Outcome::of(c.status, &c.code) == Outcome::NotBuilt)
    {
        unbuilt += 1;
        if !c.action.is_empty() && !not_built.contains(&c.action) {
            not_built.push(c.action);
        }
    }
    if unbuilt > 0 {
        let mut named = not_built
            .iter()
            .take(MOST_NAMED)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        if not_built.len() > MOST_NAMED {
            named.push_str(", …");
        }
        out.push(Signal {
            kind: SignalKind::NotBuilt,
            count: unbuilt,
            title: format!(
                "{} not built",
                plural(unbuilt, "call to an operation", "calls to operations")
            ),
            detail: named,
            href: Some("/live?outcome=not-built".to_owned()),
            resource: None,
        });
    }
    out.extend(bursts(app, s));
    out
}

/// A service whose calls failed [`BURST`] or more times within [`BURST_MS`] of its newest failure.
fn bursts(app: &[&Call], s: &Shared) -> Vec<Signal> {
    // By service: the newest error's time, the errors near it, and the codes counted.
    let mut by: BTreeMap<&str, (u64, u64, HashMap<&str, u64>)> = BTreeMap::new();
    for c in app
        .iter()
        .filter(|c| Outcome::of(c.status, &c.code) == Outcome::Error)
    {
        let service = s.facts.service_named(c.signing_name);
        let entry = by.entry(service).or_insert((c.at_ms, 0, HashMap::new()));
        if entry.0.saturating_sub(c.at_ms) <= BURST_MS {
            entry.1 += 1;
            *entry.2.entry(&c.code).or_default() += 1;
        }
    }
    by.into_iter()
        .filter(|(service, (_, n, _))| *n >= BURST && !service.is_empty())
        .map(|(service, (_, n, codes))| {
            let code = codes
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))
                .map(|(code, _)| code)
                .unwrap_or_default();
            Signal {
                kind: SignalKind::Errors,
                count: n,
                title: format!("{n} {} errors", title_of(s, service)),
                detail: if code.is_empty() {
                    "within five minutes".to_owned()
                } else {
                    format!("mostly {code}, within five minutes")
                },
                href: Some(format!("/live?outcome=error&service={service}")),
                resource: None,
            }
        })
        .collect()
}

/// A service's title, by its `--services` name.
fn title_of(s: &Shared, service: &str) -> String {
    s.facts
        .services
        .iter()
        .find(|v| v.name == service)
        .map_or_else(|| service.to_owned(), |v| v.title.to_owned())
}

/// Seconds without a request before a service lets go (`--idle-secs`, 300 unless the run says otherwise).
fn idle_secs(s: &Shared) -> u64 {
    s.facts
        .flags
        .iter()
        .find(|f| f.name == "idle-secs")
        .and_then(|f| f.value.as_deref()?.parse().ok())
        .unwrap_or(300)
}

/// The services any call reached (an app's, this console's, another service's on the bus), newest first: what keeps a service awake.
fn activity(calls: &[Call], s: &Shared) -> Vec<ServiceSeen> {
    let mut out: Vec<ServiceSeen> = Vec::new();
    for c in calls.iter().rev() {
        let service = s.facts.service_named(c.signing_name);
        if service.is_empty() {
            continue;
        }
        match out.iter_mut().find(|v| v.service == service) {
            Some(v) => v.calls += 1,
            None => out.push(ServiceSeen {
                service: service.to_owned(),
                title: title_of(s, service),
                calls: 1,
                last_ms: c.at_ms,
            }),
        }
    }
    out
}

/// `GET /api/v1/home`.
pub async fn home(State(s): State<Shared>) -> Response {
    let calls = s.recorder.newest(ALL).calls;
    let app: Vec<&Call> = calls.iter().rev().filter(|c| from_app(c)).collect();
    let mut callers: Vec<(String, u64, u64, Vec<String>)> = Vec::new();
    let mut services: Vec<ServiceSeen> = Vec::new();
    for c in &app {
        if !c.caller.is_empty() {
            match callers.iter_mut().find(|(k, ..)| **k == *c.caller) {
                Some((_, n, _, sdks)) => {
                    *n += 1;
                    if !c.sdk.is_empty()
                        && sdks.len() < MOST_SDKS
                        && !sdks.iter().any(|x| **x == *c.sdk)
                    {
                        sdks.push(c.sdk.to_string());
                    }
                }
                None => callers.push((
                    c.caller.to_string(),
                    1,
                    c.at_ms,
                    (!c.sdk.is_empty())
                        .then(|| c.sdk.to_string())
                        .into_iter()
                        .collect(),
                )),
            }
        }
        let service = s.facts.service_named(c.signing_name);
        if service.is_empty() {
            continue;
        }
        match services.iter_mut().find(|v| v.service == service) {
            Some(v) => v.calls += 1,
            None => services.push(ServiceSeen {
                service: service.to_owned(),
                title: title_of(&s, service),
                calls: 1,
                last_ms: c.at_ms,
            }),
        }
    }
    let named = iam::principals(&s.facts, callers.iter().map(|c| c.0.as_str())).await;
    let view = HomeView {
        connected: !app.is_empty(),
        callers: callers
            .into_iter()
            .map(|(key, calls, last_ms, sdks)| CallerSeen {
                principal: iam::principal_of(&named, &key),
                sdks,
                calls,
                last_ms,
            })
            .collect(),
        services,
        activity: activity(&calls, &s),
        idle_secs: idle_secs(&s),
        recent: app
            .iter()
            .take(RECENT)
            .map(|c| CallRecord::new(c, &s.facts))
            .collect(),
        signals: signals(&app, &s),
        ring: fill(&s),
    };
    json(StatusCode::OK, &view)
}
