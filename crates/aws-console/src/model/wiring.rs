//! What Live's map is drawn from.

use serde::Serialize;

/// One resource on Live's map: a thing a service holds that an app called or that is wired to one that was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WireNode {
    /// `<service>/<kind>/<name>`: what an edge names, and what the Resources space opens (`sqs/queues/pick-lists`).
    pub id: String,
    /// The service by its `--services` name.
    pub service: String,
    /// The kind's address in the Resources space (`queues`).
    pub kind: String,
    pub name: String,
    /// What it holds now, in words (`2 waiting · 1 in flight`); `null` when the service says nothing of the kind.
    pub sentence: Option<String>,
}

/// A configured line between two resources: a rule's target, a dead-letter queue, a subscription, a bucket's notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WireEdge {
    pub from: String,
    pub to: String,
    /// What the line is (`target`, `redrive`, `subscription`, `notify`, `polling`).
    pub label: String,
    /// More of it (`after 5 receives`, `ObjectCreated`); `null` when there is none.
    pub note: Option<String>,
}

/// `GET /api/v1/live/wiring`: the resources of the services apps called in this run and how they are wired together, read
/// through the bus as the console. A service no app called is not looked in, so none is woken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Wiring {
    pub nodes: Vec<WireNode>,
    pub edges: Vec<WireEdge>,
    /// The services looked in.
    pub looked: Vec<String>,
    /// More resources than the map holds: the rest are not drawn.
    pub more: bool,
    /// A read that could not be made, in a sentence each.
    pub failed: Vec<String>,
}
