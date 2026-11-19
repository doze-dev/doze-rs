//! The REST API (v1) control plane. Every operation of the model is matched here: built, or refused by name (`built`).

mod apis;
mod docs;
mod extras;
mod import_keys;
mod keys;
mod methods;
mod paging;
mod stages;

use aws_model_api_gateway::Operation;
use serde_json::Value;

use crate::core::Core;
use crate::doc::Doc;
use crate::errors::{no_api, refused};
use crate::req::{Req, Res};

pub(crate) use apis::{new_api, valid_part};
pub(crate) use docs::export as documentation;
pub(crate) use extras::new_model;
pub(crate) use paging::page;
pub(crate) use stages::snapshot;

/// What `GetRestApi` shows of a stored API.
pub(crate) fn view(doc: &Value) -> Value {
    Value::Object(apis::api_view(doc))
}

/// The operations not built, and why (`docs/aws/compatibility-notes.toml`): domain-name access associations need VPC endpoints;
/// SDK generation and the SDK types are not planned.
pub(crate) fn built(op: Operation) -> bool {
    !matches!(
        op,
        Operation::CreateDomainNameAccessAssociation
            | Operation::DeleteDomainNameAccessAssociation
            | Operation::GetDomainNameAccessAssociations
            | Operation::RejectDomainNameAccessAssociation
            | Operation::GetSdk
            | Operation::GetSdkType
            | Operation::GetSdkTypes
    )
}

/// Client certificates, VPC links and the test invocations.
fn others(core: &mut Core, op: Operation, r: &Req) -> Option<Res> {
    let name = format!("{op:?}");
    Some(match op {
        Operation::GenerateClientCertificate => crate::certs::generate(core, r),
        Operation::GetClientCertificate
        | Operation::GetClientCertificates
        | Operation::UpdateClientCertificate
        | Operation::DeleteClientCertificate => crate::certs::op(core, &name, r),
        Operation::CreateVpcLink => crate::links::create_v1(core, r),
        Operation::GetVpcLink
        | Operation::GetVpcLinks
        | Operation::UpdateVpcLink
        | Operation::DeleteVpcLink => crate::links::v1(core, &name, r),
        Operation::TestInvokeMethod => crate::plane::test::find_method(core, r),
        Operation::TestInvokeAuthorizer => crate::plane::test::find_authorizer(core, r),
        _ => return None,
    })
}

/// Runs operation `op`.
pub(crate) fn dispatch(core: &mut Core, op: u32, r: &Req) -> Res {
    let Some(op) = Operation::from_index(op) else {
        return Err(refused("This operation"));
    };
    if !built(op) {
        return Err(refused(&format!("{op:?}")));
    }
    if let Some(res) = apis::dispatch(core, op, r) {
        return res;
    }
    if let Some(res) = methods::dispatch(core, op, r) {
        return res;
    }
    if let Some(res) = stages::dispatch(core, op, r) {
        return res;
    }
    if let Some(res) = extras::dispatch(core, op, r) {
        return res;
    }
    if let Some(res) = docs::dispatch(core, op, r) {
        return res;
    }
    if let Some(res) = others(core, op, r) {
        return res;
    }
    keys::dispatch(core, op, r).unwrap_or_else(|| Err(refused(&format!("{op:?}"))))
}

/// The REST API the request names (`restApiId`).
pub(crate) fn api<'a>(core: &'a Core, r: &Req) -> Result<&'a Value, aws_wire::ApiError> {
    let id = r.s("restApiId");
    core.rest
        .get(id)
        .map(|d| &d.0)
        .ok_or_else(|| no_api(r.account(), id))
}

/// The REST API the request names, to change.
pub(crate) fn api_mut<'a>(
    core: &'a mut Core,
    r: &Req,
) -> Result<&'a mut Value, aws_wire::ApiError> {
    let id = r.s("restApiId");
    match core.rest.get_mut(id) {
        Some(Doc(v)) => Ok(v),
        None => Err(no_api(r.account(), id)),
    }
}

/// The ARN of a v1 resource path (`/restapis/abc`), as tags and IAM name it.
pub(crate) fn arn(region: &str, path: &str) -> String {
    format!("arn:aws:apigateway:{region}::{path}")
}
