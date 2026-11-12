//! AlterConfigs (replaces what a resource has) and IncrementalAlterConfigs (changes it): both answer for each resource, in
//! the order asked, and change what the broker does from then on.

mod resource;
pub(crate) use resource::{BROKER, BROKER_LOGGER, CLIENT_METRICS, GROUP, TOPIC, unsupported};

use kafka_storage::fs::Fs;
use kafka_wire::StrBytes;
use kafka_wire::api::Response;
use kafka_wire::messages::alter_configs_request::AlterConfigsResource as LegacyResource;
use kafka_wire::messages::alter_configs_response::AlterConfigsResourceResponse as LegacyAnswer;
use kafka_wire::messages::incremental_alter_configs_request::AlterConfigsResource as IncrementalResource;
use kafka_wire::messages::incremental_alter_configs_response::AlterConfigsResourceResponse as IncrementalAnswer;
use kafka_wire::messages::{
    AlterConfigsRequest, AlterConfigsResponse, IncrementalAlterConfigsRequest,
    IncrementalAlterConfigsResponse,
};

use crate::configs::{Change, Op, Refusal};
use crate::{Broker, Caller, Outcome};
use resource::Resource;

fn message(refusal: &Refusal) -> Option<StrBytes> {
    Some(StrBytes::from_string(refusal.message.clone()))
}

fn code(refusal: &Option<Refusal>) -> i16 {
    refusal.as_ref().map_or(0, |r| r.code.code())
}

impl<F: Fs> Broker<F> {
    pub(crate) async fn alter_configs(
        &self,
        caller: &Caller,
        req: &AlterConfigsRequest,
    ) -> Outcome {
        let resources: Vec<Resource> = req.resources.iter().map(legacy).collect();
        let results = self
            .alter_resources(caller, &resources, req.validate_only, true)
            .await;
        let mut resp = AlterConfigsResponse::default();
        for (r, result) in req.resources.iter().zip(&results) {
            let mut a = LegacyAnswer::default();
            a.error_code = code(result);
            a.error_message = result.as_ref().and_then(message);
            a.resource_type = r.resource_type;
            a.resource_name = r.resource_name.clone();
            resp.responses.push(a);
        }
        Outcome::Respond(Response::AlterConfigs(resp))
    }

    pub(crate) async fn incremental_alter_configs(
        &self,
        caller: &Caller,
        req: &IncrementalAlterConfigsRequest,
    ) -> Outcome {
        let resources: Vec<Resource> = req.resources.iter().map(incremental).collect();
        let results = self
            .alter_resources(caller, &resources, req.validate_only, false)
            .await;
        let mut resp = IncrementalAlterConfigsResponse::default();
        for (r, result) in req.resources.iter().zip(&results) {
            let mut a = IncrementalAnswer::default();
            a.error_code = code(result);
            a.error_message = result.as_ref().and_then(message);
            a.resource_type = r.resource_type;
            a.resource_name = r.resource_name.clone();
            resp.responses.push(a);
        }
        Outcome::Respond(Response::IncrementalAlterConfigs(resp))
    }
}

fn legacy(r: &LegacyResource) -> Resource {
    Resource {
        kind: r.resource_type,
        name: r.resource_name.to_string(),
        changes: r
            .configs
            .iter()
            .map(|c| Change {
                name: c.name.to_string(),
                op: Op::Set,
                value: c.value.as_ref().map(ToString::to_string),
            })
            .collect(),
        bad_op: None,
    }
}

fn incremental(r: &IncrementalResource) -> Resource {
    let mut bad_op = None;
    let changes = r
        .configs
        .iter()
        .map(|c| Change {
            name: c.name.to_string(),
            op: Op::from_wire(c.config_operation).unwrap_or_else(|| {
                bad_op = Some(c.config_operation);
                Op::Set
            }),
            value: c.value.as_ref().map(ToString::to_string),
        })
        .collect();
    Resource {
        kind: r.resource_type,
        name: r.resource_name.to_string(),
        changes,
        bad_op,
    }
}
