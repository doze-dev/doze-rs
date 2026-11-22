//! What each service says of the resources it holds and where they send: one reader a service, over the bus as the console.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{Graph, MOST_EACH, arn_node, last, num, plural, strings};
use crate::api::at;
use crate::resources::bus::Bus;

pub(super) async fn sqs(bus: &Bus, g: &mut Graph) -> Result<(), String> {
    let listed = bus
        .call("sqs", "ListQueues", &json!({"MaxResults": 1000}))
        .await
        .map_err(|e| e.message)?;
    // A queue: its name, messages waiting, in flight, and the dead-letter queue its redrive policy names with the limit.
    type Queue = (String, u64, u64, Option<(String, u64)>);
    let mut queues: Vec<Queue> = Vec::new();
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
            g.line_to_arn(
                &id,
                arn,
                "redrive",
                Some(format!("after {}", plural(*max, "receive", "receives"))),
            );
        }
    }
    Ok(())
}

pub(super) async fn sns(bus: &Bus, g: &mut Graph) -> Result<(), String> {
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
                g.line_to_arn(
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

pub(super) async fn events(bus: &Bus, g: &mut Graph) -> Result<(), String> {
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
            g.line_to_arn(&id, arn, "target", Some(rule.clone()));
        }
    }
    Ok(())
}

pub(super) async fn s3(bus: &Bus, g: &mut Graph) -> Result<(), String> {
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
            g.line_to_arn(
                &id,
                arn,
                "notify",
                (!event.is_empty()).then(|| event.clone()),
            );
        }
    }
    Ok(())
}

pub(super) async fn lambda(bus: &Bus, g: &mut Graph) -> Result<(), String> {
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

pub(super) async fn dynamodb(bus: &Bus, g: &mut Graph) -> Result<(), String> {
    let t = bus
        .call("dynamodb", "ListTables", &json!({}))
        .await
        .map_err(|e| e.message)?;
    for name in strings(&t, "TableNames").take(MOST_EACH) {
        g.node("dynamodb", "tables", name, None);
    }
    Ok(())
}
