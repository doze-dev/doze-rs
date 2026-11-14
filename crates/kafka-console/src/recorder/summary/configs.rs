//! Config reads and writes. Names only: a config value can be a secret (`password`, `key`, `sasl.jaas.config`, `ssl.*`), so no
//! value is ever kept.

use kafka_wire::messages::{
    AlterConfigsRequest, AlterConfigsResponse, DescribeConfigsRequest, DescribeConfigsResponse,
    IncrementalAlterConfigsRequest, IncrementalAlterConfigsResponse,
};

use super::n;
use crate::recorder::facts::Facts;

/// The resource type names of the config APIs.
fn resource(kind: i8) -> &'static str {
    match kind {
        2 => "topic",
        4 => "broker",
        8 => "broker logger",
        _ => "resource",
    }
}

/// Names a config resource: a topic by its name, anything else as `kind name`.
fn target(f: &mut Facts, kind: i8, name: &str) {
    if kind == 2 {
        f.topic(name, []);
    } else {
        f.about(&format!("{} {}", resource(kind), name));
    }
}

pub(super) fn describe_request(r: &DescribeConfigsRequest, f: &mut Facts) {
    for res in &r.resources {
        target(f, res.resource_type, &res.resource_name);
    }
    f.asked = n(r.resources.len());
}

pub(super) fn describe_response(r: &DescribeConfigsResponse, f: &mut Facts) {
    let mut configs = 0usize;
    for res in &r.results {
        f.error(res.error_code);
        f.say(res.error_message.as_deref());
        configs += res.configs.len();
    }
    f.items = n(configs);
}

pub(super) fn alter_request(r: &AlterConfigsRequest, f: &mut Facts) {
    let mut names = 0usize;
    for res in &r.resources {
        target(f, res.resource_type, &res.resource_name);
        names += res.configs.len();
    }
    f.asked = n(names);
}

pub(super) fn alter_response(r: &AlterConfigsResponse, f: &mut Facts) {
    for res in &r.responses {
        f.error(res.error_code);
        f.say(res.error_message.as_deref());
    }
}

pub(super) fn incremental_request(r: &IncrementalAlterConfigsRequest, f: &mut Facts) {
    let mut names = 0usize;
    for res in &r.resources {
        target(f, res.resource_type, &res.resource_name);
        names += res.configs.len();
    }
    f.asked = n(names);
}

pub(super) fn incremental_response(r: &IncrementalAlterConfigsResponse, f: &mut Facts) {
    for res in &r.responses {
        f.error(res.error_code);
        f.say(res.error_message.as_deref());
    }
}
