//! `GET /api/v1/live/wiring`: the resources of the services apps called in this run, and the lines configured between them (a
//! rule's target, a dead-letter queue, a subscription, a bucket's notification, a queue a function polls), read through the bus
//! as the console. A service no app called may be asleep and a read would wake it, so none is looked in.

use std::collections::BTreeSet;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::Value;

use super::home::from_app;
use super::{at, json};
use crate::model::{WireEdge, WireNode, Wiring};
use crate::state::Shared;

/// The most resources the map holds.
const MOST: usize = 60;
/// The most of one kind read for the state each holds (one call apiece).
const MOST_EACH: usize = 40;

#[derive(Default)]
struct Graph {
    nodes: Vec<WireNode>,
    edges: Vec<WireEdge>,
    more: bool,
}

/// `service/kind/name` of what an ARN names, when it is a resource the map draws.
fn arn_node(arn: &str) -> Option<(&'static str, &'static str, String)> {
    let mut p = arn.splitn(6, ':');
    let (service, rest) = (p.nth(2)?, p.nth(2)?);
    let name = rest.rsplit([':', '/']).next().filter(|n| !n.is_empty())?;
    Some(match service {
        "sqs" => ("sqs", "queues", name.to_owned()),
        "sns" => ("sns", "topics", name.to_owned()),
        "lambda" => ("lambda", "functions", name.to_owned()),
        "s3" => ("s3", "buckets", rest.to_owned()),
        _ => return None,
    })
}

impl Graph {
    fn node(
        &mut self,
        service: &str,
        kind: &str,
        name: &str,
        sentence: Option<String>,
    ) -> Option<String> {
        let id = format!("{service}/{kind}/{name}");
        if let Some(n) = self.nodes.iter_mut().find(|n| n.id == id) {
            if sentence.is_some() {
                n.sentence = sentence;
            }
            return Some(id);
        }
        if self.nodes.len() >= MOST {
            self.more = true;
            return None;
        }
        self.nodes.push(WireNode {
            id: id.clone(),
            service: service.to_owned(),
            kind: kind.to_owned(),
            name: name.to_owned(),
            sentence,
        });
        Some(id)
    }

    fn edge(&mut self, from: &str, to: &str, label: &str, note: Option<String>) {
        if !self
            .edges
            .iter()
            .any(|e| e.from == from && e.to == to && e.label == label && e.note == note)
        {
            self.edges.push(WireEdge {
                from: from.to_owned(),
                to: to.to_owned(),
                label: label.to_owned(),
                note,
            });
        }
    }

    /// A line to what `arn` names, which becomes a node when it is not one yet.
    fn line_to_arn(&mut self, from: &str, arn: &str, label: &str, note: Option<String>) {
        if let Some((service, kind, name)) = arn_node(arn)
            && let Some(to) = self.node(service, kind, &name, None)
        {
            self.edge(from, &to, label, note);
        }
    }
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn num(v: &Value) -> u64 {
    v.as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| v.as_u64())
        .unwrap_or(0)
}

fn strings<'a>(v: &'a Value, key: &str) -> impl Iterator<Item = &'a str> {
    at(v, key)
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

fn last(of: &str) -> &str {
    of.rsplit([':', '/']).next().unwrap_or(of)
}

mod read;

/// `GET /api/v1/live/wiring`.
pub async fn wiring(State(s): State<Shared>) -> Response {
    let calls = s.recorder.newest(usize::MAX).calls;
    let called: BTreeSet<&str> = calls
        .iter()
        .filter(|c| from_app(c))
        .map(|c| s.facts.service_named(c.signing_name))
        .collect();
    let mut g = Graph::default();
    let mut out = Wiring {
        nodes: Vec::new(),
        edges: Vec::new(),
        looked: Vec::new(),
        more: false,
        failed: Vec::new(),
    };
    let Some(bus) = s.bus() else {
        out.failed.push(
            "This console was started without the services' bus, so it cannot look.".to_owned(),
        );
        return json(StatusCode::OK, &out);
    };
    // Queues first: a dead-letter queue is named by the queue that uses it, and a function by the queue it polls.
    for service in ["sqs", "sns", "eventbridge", "s3", "lambda", "dynamodb"] {
        if !called.contains(service) || !crate::resources::view::served(&s.facts, service) {
            continue;
        }
        let found = match service {
            "sqs" => read::sqs(&bus, &mut g).await,
            "sns" => read::sns(&bus, &mut g).await,
            "eventbridge" => read::events(&bus, &mut g).await,
            "s3" => read::s3(&bus, &mut g).await,
            "lambda" => read::lambda(&bus, &mut g).await,
            _ => read::dynamodb(&bus, &mut g).await,
        };
        out.looked.push(service.to_owned());
        if let Err(why) = found {
            out.failed.push(format!("{service}: {why}"));
        }
    }
    out.nodes = g.nodes;
    out.edges = g.edges;
    out.more = g.more;
    json(StatusCode::OK, &out)
}
