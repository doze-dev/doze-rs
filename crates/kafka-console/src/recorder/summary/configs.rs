//! Config reads and writes. Names only: a config value can be a secret (`password`, `key`, `sasl.jaas.config`, `ssl.*`), so no
//! value is ever kept.

use kafka_wire::messages::{
    AlterClientQuotasRequest, AlterClientQuotasResponse, AlterConfigsRequest, AlterConfigsResponse,
    DescribeClientQuotasRequest, DescribeClientQuotasResponse, DescribeConfigsRequest,
    DescribeConfigsResponse, IncrementalAlterConfigsRequest, IncrementalAlterConfigsResponse,
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

pub(super) fn describe_quotas_request(r: &DescribeClientQuotasRequest, f: &mut Facts) {
    f.about("client quotas");
    f.asked = n(r.components.len());
}

/// The code and message, and how many entities came back (who they are and their values are not kept).
pub(super) fn describe_quotas_response(r: &DescribeClientQuotasResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.say(r.error_message.as_deref().filter(|m| !m.is_empty()));
    f.items = n(r.entries.as_ref().map_or(0, Vec::len));
}

pub(super) fn alter_quotas_request(r: &AlterClientQuotasRequest, f: &mut Facts) {
    f.about("client quotas");
    f.asked = n(r.entries.len());
}

pub(super) fn alter_quotas_response(r: &AlterClientQuotasResponse, f: &mut Facts) {
    for e in &r.entries {
        f.error(e.error_code);
        f.say(e.error_message.as_deref());
    }
    f.items = n(r.entries.len());
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
