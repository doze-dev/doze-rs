//! `GET /api/v1/home/checks`: what the services hold that needs a look, read through the bus as the console from the services
//! apps called in this run (a service no app called may be asleep, and a check would wake it).

use std::collections::BTreeSet;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Value, json};

use super::{ALL, from_app, plural};
use crate::api::{at, json};
use crate::model::{HomeChecks, ResourceLink, Signal, SignalKind};
use crate::resources::bus::Bus;
use crate::state::Shared;

/// The most queues or state machines one check reads, each with one call more.
const MOST_EACH: usize = 100;

/// The statuses of a stack that failed and was not cleaned up.
const FAILED_STACKS: &[&str] = &[
    "CREATE_FAILED",
    "ROLLBACK_FAILED",
    "ROLLBACK_COMPLETE",
    "DELETE_FAILED",
    "UPDATE_FAILED",
    "UPDATE_ROLLBACK_FAILED",
    "UPDATE_ROLLBACK_COMPLETE",
    "IMPORT_ROLLBACK_FAILED",
    "IMPORT_ROLLBACK_COMPLETE",
];

fn link(service: &str, kind: &str, name: &str) -> Option<ResourceLink> {
    Some(ResourceLink {
        service: service.to_owned(),
        kind: kind.to_owned(),
        name: name.to_owned(),
    })
}

/// The name at the end of an ARN or a URL.
fn last(of: &str) -> &str {
    of.rsplit([':', '/']).next().unwrap_or(of)
}

/// Dead-letter queues (named by another queue's redrive policy) holding messages.
async fn dead_letters(bus: &Bus) -> Result<Vec<Signal>, String> {
    let listed = bus
        .call("sqs", "ListQueues", &json!({"MaxResults": 1000}))
        .await
        .map_err(|e| e.message)?;
    let mut queues: Vec<Queue> = Vec::new();
    for url in at(&listed, "QueueUrls")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .take(MOST_EACH)
    {
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
        let target = at(a, "RedrivePolicy")
            .as_str()
            .and_then(|p| serde_json::from_str::<Value>(p).ok())
            .and_then(|p| at(&p, "deadLetterTargetArn").as_str().map(str::to_owned));
        let held = at(a, "ApproximateNumberOfMessages")
            .as_str()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        queues.push((
            url.to_owned(),
            at(a, "QueueArn").as_str().unwrap_or_default().to_owned(),
            held,
            target,
        ));
    }
    Ok(dead_letter_signals(&queues))
}

/// What a queue is read as: its URL, its ARN, the messages it holds, the dead-letter queue its redrive policy names.
type Queue = (String, String, u64, Option<String>);

/// The signals of the queues that another queue names as its dead-letter queue and that hold messages. The page opens a queue by
/// its NAME (the end of its URL): the console's API answers 400 to a URL.
fn dead_letter_signals(queues: &[Queue]) -> Vec<Signal> {
    let targets: BTreeSet<&str> = queues.iter().filter_map(|q| q.3.as_deref()).collect();
    queues
        .iter()
        .filter(|(_, arn, held, _)| *held > 0 && targets.contains(arn.as_str()))
        .map(|(url, _, held, _)| Signal {
            kind: SignalKind::DeadLetters,
            count: *held,
            title: format!(
                "{} in a dead-letter queue",
                plural(*held, "message", "messages")
            ),
            detail: last(url).to_owned(),
            href: None,
            resource: link("sqs", "queues", last(url)),
        })
        .collect()
}

async fn alarms(bus: &Bus) -> Result<Vec<Signal>, String> {
    let got = bus
        .call(
            "monitoring",
            "DescribeAlarms",
            &json!({"StateValue": "ALARM"}),
        )
        .await
        .map_err(|e| e.message)?;
    Ok(at(&got, "MetricAlarms")
        .as_array()
        .into_iter()
        .flatten()
        .chain(at(&got, "CompositeAlarms").as_array().into_iter().flatten())
        .filter_map(|a| at(a, "AlarmName").as_str())
        .map(|name| Signal {
            kind: SignalKind::Alarm,
            count: 1,
            title: "An alarm is in ALARM".to_owned(),
            detail: name.to_owned(),
            href: None,
            resource: link("cloudwatch", "alarms", name),
        })
        .collect())
}

async fn failed_executions(bus: &Bus) -> Result<Vec<Signal>, String> {
    let machines = bus
        .call("states", "ListStateMachines", &json!({"maxResults": 1000}))
        .await
        .map_err(|e| e.message)?;
    let mut out = Vec::new();
    for m in at(&machines, "stateMachines")
        .as_array()
        .into_iter()
        .flatten()
        .take(MOST_EACH)
    {
        let Some(arn) = at(m, "stateMachineArn").as_str() else {
            continue;
        };
        let input = json!({"stateMachineArn": arn, "statusFilter": "FAILED", "maxResults": 1000});
        let Ok(got) = bus.call("states", "ListExecutions", &input).await else {
            continue;
        };
        let n = at(&got, "executions").as_array().map_or(0, Vec::len) as u64;
        if n > 0 {
            out.push(Signal {
                kind: SignalKind::FailedExecutions,
                count: n,
                title: format!("{} failed", plural(n, "execution", "executions")),
                detail: at(m, "name")
                    .as_str()
                    .unwrap_or_else(|| last(arn))
                    .to_owned(),
                href: None,
                resource: link("stepfunctions", "machines", last(arn)),
            });
        }
    }
    Ok(out)
}

async fn failed_stacks(bus: &Bus) -> Result<Vec<Signal>, String> {
    let got = bus
        .call(
            "cloudformation",
            "ListStacks",
            &json!({"StackStatusFilter": FAILED_STACKS}),
        )
        .await
        .map_err(|e| e.message)?;
    Ok(at(&got, "StackSummaries")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| Some((at(s, "StackName").as_str()?, at(s, "StackStatus").as_str()?)))
        .map(|(name, status)| Signal {
            kind: SignalKind::FailedStack,
            count: 1,
            title: format!("A stack is {status}"),
            detail: name.to_owned(),
            href: None,
            resource: link("cloudformation", "stacks", name),
        })
        .collect())
}

/// The protocols SNS keeps a subscription of and delivers nothing to (no mail server, no phone network, no push service, no
/// Data Firehose): what `crates/aws-sns/src/deliver.rs` does not deliver.
const STORED_ONLY: &[&str] = &["email", "email-json", "sms", "application", "firehose"];

/// Subscriptions a script made that doze-aws keeps and delivers nothing to.
async fn stored_only(bus: &Bus) -> Result<Vec<Signal>, String> {
    let got = bus
        .call("sns", "ListSubscriptions", &json!({}))
        .await
        .map_err(|e| e.message)?;
    let n = at(&got, "Subscriptions")
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| {
            at(s, "Protocol")
                .as_str()
                .is_some_and(|p| STORED_ONLY.contains(&p))
        })
        .count() as u64;
    Ok(if n == 0 {
        Vec::new()
    } else {
        vec![Signal {
            kind: SignalKind::NotActive,
            count: n,
            title: "Not active locally".to_owned(),
            detail: format!("{} stored", plural(n, "subscription", "subscriptions")),
            href: Some("/resources/sns/subscriptions".to_owned()),
            resource: None,
        }]
    })
}

/// API Gateway custom domain names and VPC links: stored and listed, and nothing is routed through them locally.
async fn stored_gateway(bus: &Bus) -> Result<Vec<Signal>, String> {
    let mut parts = Vec::new();
    let mut count = 0;
    for (op, one, many) in [
        ("GetDomainNames", "domain name", "domain names"),
        ("GetVpcLinks", "VPC link", "VPC links"),
    ] {
        let got = bus
            .call("apigateway", op, &json!({}))
            .await
            .map_err(|e| e.message)?;
        let n = at(&got, "item").as_array().map_or(0, Vec::len) as u64;
        if n > 0 {
            count += n;
            parts.push(plural(n, one, many));
        }
    }
    Ok(if count == 0 {
        Vec::new()
    } else {
        vec![Signal {
            kind: SignalKind::NotActive,
            count,
            title: "Not active locally".to_owned(),
            detail: format!("{} stored", parts.join(" and ")),
            href: Some("/resources/apigateway".to_owned()),
            resource: None,
        }]
    })
}

/// `GET /api/v1/home/checks`.
pub async fn checks(State(s): State<Shared>) -> Response {
    let calls = s.recorder.newest(ALL).calls;
    let called: BTreeSet<&str> = calls
        .iter()
        .filter(|c| from_app(c))
        .map(|c| s.facts.service_named(c.signing_name))
        .collect();
    let mut out = HomeChecks {
        checked: Vec::new(),
        signals: Vec::new(),
        failed: Vec::new(),
    };
    let Some(bus) = s.bus() else {
        out.failed.push(
            "This console was started without the services' bus, so it cannot look.".to_owned(),
        );
        return json(StatusCode::OK, &out);
    };
    for service in [
        "sqs",
        "sns",
        "apigateway",
        "cloudwatch",
        "stepfunctions",
        "cloudformation",
    ] {
        if !called.contains(service) || !crate::resources::view::served(&s.facts, service) {
            continue;
        }
        let found = match service {
            "sqs" => dead_letters(&bus).await,
            "sns" => stored_only(&bus).await,
            "apigateway" => stored_gateway(&bus).await,
            "cloudwatch" => alarms(&bus).await,
            "stepfunctions" => failed_executions(&bus).await,
            _ => failed_stacks(&bus).await,
        };
        out.checked.push(service.to_owned());
        match found {
            Ok(signals) => out.signals.extend(signals),
            Err(why) => out.failed.push(format!("{service}: {why}")),
        }
    }
    json(StatusCode::OK, &out)
}

#[cfg(test)]
mod tests {
    use super::{Queue, dead_letter_signals, last};

    #[test]
    fn a_name_is_the_end_of_its_arn_or_url() {
        assert_eq!(
            last("http://127.0.0.1:4566/000000000000/jobs-dlq"),
            "jobs-dlq"
        );
        assert_eq!(last("arn:aws:states:us-east-1:1:stateMachine:flow"), "flow");
        assert_eq!(last("plain"), "plain");
    }

    #[test]
    fn a_dead_letter_signal_opens_the_queue_by_its_name_not_its_url() {
        let url = "http://127.0.0.1:4566/000000000000/jobs-dlq";
        let arn = "arn:aws:sqs:us-east-1:000000000000:jobs-dlq";
        let queues: Vec<Queue> = vec![
            (url.to_owned(), arn.to_owned(), 1, None),
            (
                "http://127.0.0.1:4566/000000000000/jobs".to_owned(),
                "arn:aws:sqs:us-east-1:000000000000:jobs".to_owned(),
                0,
                Some(arn.to_owned()),
            ),
        ];
        let signals = dead_letter_signals(&queues);
        assert_eq!(signals.len(), 1);
        let link = signals[0].resource.as_ref().expect("a signal opens a page");
        assert_eq!(
            (
                link.service.as_str(),
                link.kind.as_str(),
                link.name.as_str()
            ),
            ("sqs", "queues", "jobs-dlq")
        );
        assert_eq!(signals[0].detail, "jobs-dlq");
    }
}
