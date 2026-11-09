//! Requests for DescribeConfigs, AlterConfigs and IncrementalAlterConfigs.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_arguments,
    clippy::type_complexity
)]

use super::Rig;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::alter_configs_request::{AlterConfigsResource, AlterableConfig};
use kafka_wire::messages::describe_configs_request::DescribeConfigsResource;
use kafka_wire::messages::describe_configs_response::{
    DescribeConfigsResourceResult, DescribeConfigsResult,
};
use kafka_wire::messages::incremental_alter_configs_request::{
    AlterConfigsResource as IncrementalResource, AlterableConfig as IncrementalConfig,
};
use kafka_wire::messages::{
    AlterConfigsRequest, DescribeConfigsRequest, IncrementalAlterConfigsRequest,
};

pub const TOPIC: i8 = 2;
pub const BROKER: i8 = 4;

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// One change of IncrementalAlterConfigs: name, operation (0 set, 1 delete, 2 append, 3 subtract), value.
pub type Op<'a> = (&'a str, i8, Option<&'a str>);

/// What an alter answered for one resource: the code and the message.
pub type Answer = (i16, Option<String>);

impl Rig {
    /// DescribeConfigs of one resource, at version `v`.
    pub async fn describe_configs(
        &self,
        v: i16,
        kind: i8,
        name: &str,
        keys: Option<&[&str]>,
        synonyms: bool,
        documentation: bool,
    ) -> DescribeConfigsResult {
        let mut req = DescribeConfigsRequest::default();
        let mut r = DescribeConfigsResource::default();
        r.resource_type = kind;
        r.resource_name = text(name);
        r.configuration_keys = keys.map(|k| k.iter().map(|k| text(k)).collect());
        req.resources.push(r);
        req.include_synonyms = synonyms;
        req.include_documentation = documentation;
        let Response::DescribeConfigs(resp) = self.ask(v, Request::DescribeConfigs(req)).await
        else {
            panic!("not DescribeConfigs")
        };
        resp.results.into_iter().next().unwrap()
    }

    /// One config of a topic, or of the broker, by name.
    pub async fn config_of(
        &self,
        kind: i8,
        name: &str,
        key: &str,
    ) -> Option<DescribeConfigsResourceResult> {
        let r = self
            .describe_configs(4, kind, name, Some(&[key]), true, false)
            .await;
        r.configs.into_iter().next()
    }

    /// IncrementalAlterConfigs of the resources, at version `v`: (type, name, changes). One answer for each.
    pub async fn incremental(
        &self,
        v: i16,
        validate_only: bool,
        resources: &[(i8, &str, Vec<Op<'_>>)],
    ) -> Vec<Answer> {
        let mut req = IncrementalAlterConfigsRequest::default();
        req.validate_only = validate_only;
        for (kind, name, ops) in resources {
            let mut r = IncrementalResource::default();
            r.resource_type = *kind;
            r.resource_name = text(name);
            for (n, op, value) in ops {
                let mut c = IncrementalConfig::default();
                c.name = text(n);
                c.config_operation = *op;
                c.value = value.map(text);
                r.configs.push(c);
            }
            req.resources.push(r);
        }
        let Response::IncrementalAlterConfigs(resp) =
            self.ask(v, Request::IncrementalAlterConfigs(req)).await
        else {
            panic!("not IncrementalAlterConfigs")
        };
        resp.responses
            .into_iter()
            .map(|r| (r.error_code, r.error_message.map(|m| m.to_string())))
            .collect()
    }

    /// AlterConfigs (legacy) of the resources, at version `v`: (type, name, configs).
    pub async fn alter(
        &self,
        v: i16,
        validate_only: bool,
        resources: &[(i8, &str, Vec<(&str, Option<&str>)>)],
    ) -> Vec<Answer> {
        let mut req = AlterConfigsRequest::default();
        req.validate_only = validate_only;
        for (kind, name, configs) in resources {
            let mut r = AlterConfigsResource::default();
            r.resource_type = *kind;
            r.resource_name = text(name);
            for (n, value) in configs {
                let mut c = AlterableConfig::default();
                c.name = text(n);
                c.value = value.map(text);
                r.configs.push(c);
            }
            req.resources.push(r);
        }
        let Response::AlterConfigs(resp) = self.ask(v, Request::AlterConfigs(req)).await else {
            panic!("not AlterConfigs")
        };
        resp.responses
            .into_iter()
            .map(|r| (r.error_code, r.error_message.map(|m| m.to_string())))
            .collect()
    }
}
