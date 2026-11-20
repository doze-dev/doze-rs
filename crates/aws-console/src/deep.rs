//! Deep pages: the per-resource pages a descriptor's generic detail cannot be (a function's Invoke, an execution's graph, a log
//! stream's events, a stack's events and template).
//!
//! A deep page asks for what it shows by operation: the page writes the request as the service's model has it, and the console
//! makes the call through the peer bus as the caller `console` ([`crate::resources::bus::Bus`]), so Live shows it and IAM
//! decides it as any other. Only the operations in [`OPS`] can be asked for, each marked a read or a write; nothing is polled
//! or kept. Two calls are more than a pass-through: Lambda's `Invoke` ([`invoke`]: the payload in and out as text, the log tail
//! decoded) and CloudWatch Logs' `StartLiveTail` ([`tail`]: its event stream sent to the browser as server-sent events).

pub mod frames;
pub mod invoke;
pub mod tail;

use aws_wire::ApiError;
use serde_json::Value;

use crate::resources::bus::Bus;
use crate::resources::catalog;

/// One operation a deep page may ask for.
#[derive(Debug)]
pub struct Op {
    /// The service, by its name in the console's addresses (`stepfunctions`).
    pub service: &'static str,
    pub operation: &'static str,
    /// Whether it changes something (the page asks before it does).
    pub write: bool,
}

const fn read(service: &'static str, operation: &'static str) -> Op {
    Op {
        service,
        operation,
        write: false,
    }
}

const fn write(service: &'static str, operation: &'static str) -> Op {
    Op {
        service,
        operation,
        write: true,
    }
}

/// Every operation a deep page may ask for.
pub static OPS: &[Op] = &[
    read("lambda", "GetFunctionConfiguration"),
    read("lambda", "ListVersionsByFunction"),
    read("lambda", "ListAliases"),
    write("lambda", "UpdateFunctionConfiguration"),
    write("lambda", "Invoke"),
    read("stepfunctions", "DescribeStateMachine"),
    read("stepfunctions", "DescribeExecution"),
    read("stepfunctions", "DescribeStateMachineForExecution"),
    read("stepfunctions", "GetExecutionHistory"),
    read("stepfunctions", "ListExecutions"),
    write("stepfunctions", "StartExecution"),
    write("stepfunctions", "StopExecution"),
    read("logs", "DescribeLogStreams"),
    read("logs", "GetLogEvents"),
    read("logs", "FilterLogEvents"),
    read("cloudformation", "DescribeStacks"),
    read("cloudformation", "DescribeStackEvents"),
    read("cloudformation", "GetTemplate"),
    read("cloudformation", "ListChangeSets"),
];

/// The deep page of a kind, by the id the console's pages know it by.
pub static PAGES: &[(&str, &str, &str)] = &[
    ("lambda", "functions", "lambda-function"),
    ("stepfunctions", "machines", "state-machine"),
    ("logs", "groups", "log-group"),
    ("cloudformation", "stacks", "stack"),
];

/// The deep page of `service`'s `kind`, if it has one.
#[must_use]
pub fn page_of(service: &str, kind: &str) -> Option<&'static str> {
    PAGES
        .iter()
        .find(|(s, k, _)| *s == service && *k == kind)
        .map(|(_, _, p)| *p)
}

/// The operation `operation` of `service`, when a deep page may ask for it.
#[must_use]
pub fn op(service: &str, operation: &str) -> Option<&'static Op> {
    OPS.iter()
        .find(|o| o.service == service && o.operation == operation)
}

/// `service`'s name on the bus (`stepfunctions` is `states`).
#[must_use]
pub fn target(service: &str) -> Option<&'static str> {
    catalog::descriptor(service).map(|d| d.target)
}

/// Makes `op` with `body`, as the console; the service's answer as its model has it.
///
/// # Errors
/// The service's refusal, IAM's included.
pub async fn call(bus: &Bus, op: &Op, body: &Value) -> Result<Value, ApiError> {
    let target = target(op.service)
        .ok_or_else(|| ApiError::internal(format!("{} has no descriptor", op.service)))?;
    bus.call(target, op.operation, body).await
}

#[cfg(test)]
mod tests {
    use super::{OPS, PAGES, op, page_of, target};

    #[test]
    fn every_operation_and_page_belongs_to_a_described_service() {
        for o in OPS {
            assert!(target(o.service).is_some(), "{}", o.service);
        }
        for (service, kind, _) in PAGES {
            let d = crate::resources::catalog::descriptor(service)
                .unwrap_or_else(|| panic!("{service}"));
            assert!(d.kinds.iter().any(|k| k.id == *kind), "{service}/{kind}");
        }
        assert_eq!(target("stepfunctions"), Some("states"));
    }

    #[test]
    fn only_the_named_operations_are_asked_for() {
        assert!(op("lambda", "Invoke").is_some_and(|o| o.write));
        assert!(op("logs", "GetLogEvents").is_some_and(|o| !o.write));
        assert!(op("lambda", "DeleteFunction").is_none());
        assert!(op("iam", "CreateAccessKey").is_none());
        assert_eq!(page_of("lambda", "functions"), Some("lambda-function"));
        assert_eq!(page_of("sqs", "queues"), None);
    }
}
