//! Requests for the client quota APIs.
#![allow(unreachable_pub, dead_code, clippy::unwrap_used, clippy::panic)]

use kafka_broker::Caller;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::alter_client_quotas_request::{EntityData, EntryData, OpData};
use kafka_wire::messages::{AlterClientQuotasRequest, AlterClientQuotasResponse};

use super::Rig;

pub type Entity<'a> = &'a [(&'a str, Option<&'a str>)];
/// One entity's alteration: each key set to a value or removed (`None`).
pub type Change<'a> = (Entity<'a>, &'a [(&'a str, Option<f64>)]);
/// An entity as `type=name` pairs, and its values.
pub type Listed = (Vec<String>, Vec<(String, f64)>);

pub fn alter(entries: &[Change<'_>], validate_only: bool) -> AlterClientQuotasRequest {
    let mut req = AlterClientQuotasRequest::default();
    req.validate_only = validate_only;
    for (entity, ops) in entries {
        let mut e = EntryData::default();
        e.entity = (entity.iter())
            .map(|(t, n)| {
                let mut part = EntityData::default();
                part.entity_type = StrBytes::from_string((*t).to_owned());
                part.entity_name = n.map(|n| StrBytes::from_string(n.to_owned()));
                part
            })
            .collect();
        e.ops = (ops.iter())
            .map(|(k, v)| {
                let mut op = OpData::default();
                op.key = StrBytes::from_string((*k).to_owned());
                op.value = v.unwrap_or(0.0);
                op.remove = v.is_none();
                op
            })
            .collect();
        req.entries.push(e);
    }
    req
}

pub async fn alter_as(
    rig: &Rig,
    who: &Caller,
    req: AlterClientQuotasRequest,
) -> AlterClientQuotasResponse {
    let Response::AlterClientQuotas(r) = rig.ask_as(who, 1, Request::AlterClientQuotas(req)).await
    else {
        panic!("not AlterClientQuotas")
    };
    r
}

pub const DEFAULT_CLIENT: Entity<'static> = &[("client-id", None)];
