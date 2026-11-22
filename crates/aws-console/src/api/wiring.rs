//! `GET /api/v1/live/wiring`: the resources of the services apps called in this run, and the lines configured between them (a
//! rule's target, a dead-letter queue, a subscription, a bucket's notification, a queue a function polls), read through the bus
//! as the console. A service no app called may be asleep and a read would wake it, so none is looked in.

use std::collections::BTreeSet;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Value, json};

use super::home::from_app;
use super::{at, json};
use crate::model::{WireEdge, WireNode, Wiring};
use crate::resources::bus::Bus;
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
    fn to_arn(&mut self, from: &str, arn: &str, label: &str, note: Option<String>) {
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

async fn sqs(bus: &Bus, g: &mut Graph) -> Result<(), String> {
    let listed = bus
        .call("sqs", "ListQueues", &json!({"MaxResults": 1000}))
        .await
        .map_err(|e| e.message)?;
    let mut queues: Vec<(String, u64, u64, Option<(String, u64)>)> = Vec::new();
    for url in strings(&listed, "QueueUrls").take(MOST_EACH) {
        let Ok(got) = bus
            .call(
                "sqs",
                "GetQueueAttributes",
                &json!({"QueueUrl": url, "AttributeNames": ["All"]}),
            )
            .await
        else {
            continue;
        };
        let a = at(&got, "Attributes");
        let redrive = at(a, "RedrivePolicy")
            .as_str()
            .and_then(|p| serde_json::from_str::<Value>(p).ok())
            .and_then(|p| {
                Some((
                    at(&p, "deadLetterTargetArn").as_str()?.to_owned(),
                    num(at(&p, "maxReceiveCount")),
                ))
            });
        queues.push((
            last(url).to_owned(),
            num(at(a, "ApproximateNumberOfMessages")),
            num(at(a, "ApproximateNumberOfMessagesNotVisible")),
            redrive,
        ));
    }
    let dead: BTreeSet<String> = queues
        .iter()
        .filter_map(|q| q.3.as_ref().map(|(arn, _)| last(arn).to_owned()))
        .collect();
    for (name, waiting, flight, redrive) in &queues {
        let sentence = if dead.contains(name) && *waiting > 0 {
            plural(*waiting, "dead letter", "dead letters")
        } else {
            format!("{waiting} waiting · {flight} in flight")
        };
        let Some(id) = g.node("sqs", "queues", name, Some(sentence)) else {
            continue;
        };
        if let Some((arn, max)) = redrive {
            g.to_arn(
                &id,
                arn,
                "redrive",
                Some(format!("after {}", plural(*max, "receive", "receives"))),
            );
        }
    }
    Ok(())
}

async fn sns(bus: &Bus, g: &mut Graph) -> Result<(), String> {
    let topics = bus
        .call("sns", "ListTopics", &json!({}))
        .await
        .map_err(|e| e.message)?;
    for arn in at(&topics, "Topics")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| at(t, "TopicArn").as_str())
        .take(MOST_EACH)
    {
        let Ok(subs) = bus
            .call("sns", "ListSubscriptionsByTopic", &json!({"TopicArn": arn}))
            .await
        else {
            continue;
        };
        let rows: Vec<&Value> = at(&subs, "Subscriptions")
            .as_array()
            .into_iter()
            .flatten()
            .collect();
        let Some(id) = g.node(
            "sns",
            "topics",
            last(arn),
            Some(plural(rows.len() as u64, "subscription", "subscriptions")),
        ) else {
            continue;
        };
        for s in rows {
            if matches!(at(s, "Protocol").as_str(), Some("sqs" | "lambda")) {
                g.to_arn(
                    &id,
                    at(s, "Endpoint").as_str().unwrap_or_default(),
                    "subscription",
                    None,
                );
            }
        }
    }
    Ok(())
}

async fn events(bus: &Bus, g: &mut Graph) -> Result<(), String> {
    let buses = bus
        .call("events", "ListEventBuses", &json!({}))
        .await
        .map_err(|e| e.message)?;
    for name in at(&buses, "EventBuses")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| at(b, "Name").as_str())
        .take(MOST_EACH)
    {
        let Ok(rules) = bus
            .call("events", "ListRules", &json!({"EventBusName": name}))
            .await
        else {
            continue;
        };
        let mut targets: Vec<(String, String)> = Vec::new();
        let named: Vec<&str> = at(&rules, "Rules")
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| at(r, "Name").as_str())
            .collect();
        for rule in &named {
            let Ok(t) = bus
                .call(
                    "events",
                    "ListTargetsByRule",
                    &json!({"Rule": rule, "EventBusName": name}),
                )
                .await
            else {
                continue;
            };
            targets.extend(
                at(&t, "Targets")
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|t| at(t, "Arn").as_str())
                    .map(|arn| ((*rule).to_owned(), arn.to_owned())),
            );
        }
        // The default bus with no rule is not wired to anything: it is drawn only when an app called it.
        if named.is_empty() && name == "default" {
            continue;
        }
        let sentence = format!(
            "{} · {}",
            plural(named.len() as u64, "rule", "rules"),
            plural(targets.len() as u64, "target", "targets")
        );
        let Some(id) = g.node("eventbridge", "buses", name, Some(sentence)) else {
            continue;
        };
        for (rule, arn) in &targets {
            g.to_arn(&id, arn, "target", Some(rule.clone()));
        }
    }
    Ok(())
}

async fn s3(bus: &Bus, g: &mut Graph) -> Result<(), String> {
    let listed = bus
        .call("s3", "ListBuckets", &json!({}))
        .await
        .map_err(|e| e.message)?;
    for name in at(&listed, "Buckets")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| at(b, "Name").as_str())
        .take(MOST_EACH)
    {
        let Ok(conf) = bus
            .call(
                "s3",
                "GetBucketNotificationConfiguration",
                &json!({"Bucket": name}),
            )
            .await
        else {
            continue;
        };
        let mut lines: Vec<(&str, String)> = Vec::new();
        for (key, member) in [
            ("LambdaFunctionConfigurations", "LambdaFunctionArn"),
            ("QueueConfigurations", "QueueArn"),
            ("TopicConfigurations", "TopicArn"),
        ] {
            for c in at(&conf, key).as_array().into_iter().flatten() {
                let events: Vec<&str> = strings(c, "Events").collect();
                let first = events.first().map_or("", |e| {
                    e.trim_start_matches("s3:").split(':').next().unwrap_or("")
                });
                lines.push((at(c, member).as_str().unwrap_or_default(), first.to_owned()));
            }
        }
        let sentence = (!lines.is_empty()).then(|| {
            format!(
                "notifies {}",
                plural(lines.len() as u64, "target", "targets")
            )
        });
        let Some(id) = g.node("s3", "buckets", name, sentence) else {
            continue;
        };
        for (arn, event) in &lines {
            g.to_arn(
                &id,
                arn,
                "notify",
                (!event.is_empty()).then(|| event.clone()),
            );
        }
    }
    Ok(())
}

async fn lambda(bus: &Bus, g: &mut Graph) -> Result<(), String> {
    let fns = bus
        .call("lambda", "ListFunctions", &json!({}))
        .await
        .map_err(|e| e.message)?;
    for f in at(&fns, "Functions")
        .as_array()
        .into_iter()
        .flatten()
        .take(MOST_EACH)
    {
        if let Some(name) = at(f, "FunctionName").as_str() {
            g.node(
                "lambda",
                "functions",
                name,
                at(f, "Runtime").as_str().map(str::to_owned),
            );
        }
    }
    let maps = bus
        .call("lambda", "ListEventSourceMappings", &json!({}))
        .await
        .map_err(|e| e.message)?;
    for m in at(&maps, "EventSourceMappings")
        .as_array()
        .into_iter()
        .flatten()
    {
        let (Some(from), Some(to)) = (
            at(m, "EventSourceArn").as_str(),
            at(m, "FunctionArn").as_str(),
        ) else {
            continue;
        };
        if let (Some((fs, fk, fname)), Some((ts, tk, tname))) = (arn_node(from), arn_node(to)) {
            let (a, b) = (g.node(fs, fk, &fname, None), g.node(ts, tk, &tname, None));
            if let (Some(a), Some(b)) = (a, b) {
                g.edge(&a, &b, "polling", None);
            }
        }
    }
    Ok(())
}

async fn dynamodb(bus: &Bus, g: &mut Graph) -> Result<(), String> {
    let t = bus
        .call("dynamodb", "ListTables", &json!({}))
        .await
        .map_err(|e| e.message)?;
    for name in strings(&t, "TableNames").take(MOST_EACH) {
        g.node("dynamodb", "tables", name, None);
    }
    Ok(())
}

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
            "sqs" => sqs(&bus, &mut g).await,
            "sns" => sns(&bus, &mut g).await,
            "eventbridge" => events(&bus, &mut g).await,
            "s3" => s3(&bus, &mut g).await,
            "lambda" => lambda(&bus, &mut g).await,
            _ => dynamodb(&bus, &mut g).await,
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
