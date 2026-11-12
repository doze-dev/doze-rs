//! The REST API (v1) control plane. Every operation of the model is matched here: built, or refused by name with the reason SCOPE.md
//! gives (`built`).

mod apis;
mod extras;
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

pub(crate) use paging::page;

/// The operations not built, and why: client certificates, VPC links and domain-name access associations need network or certificate
/// infrastructure; documentation parts and versions, SDK generation and the SDK types are publishing (STAY CUT in SCOPE.md); usage is
/// not metered (`GetUsage`, `UpdateUsage`); `ImportApiKeys` reads a CSV (create keys one at a time); `TestInvoke*` and
/// `GetModelTemplate` (a VTL scaffold) are console features.
pub(crate) fn built(op: Operation) -> bool {
    !matches!(
        op,
        Operation::GenerateClientCertificate
            | Operation::GetClientCertificate
            | Operation::GetClientCertificates
            | Operation::UpdateClientCertificate
            | Operation::DeleteClientCertificate
            | Operation::CreateVpcLink
            | Operation::GetVpcLink
            | Operation::GetVpcLinks
            | Operation::UpdateVpcLink
            | Operation::DeleteVpcLink
            | Operation::CreateDomainNameAccessAssociation
            | Operation::DeleteDomainNameAccessAssociation
            | Operation::GetDomainNameAccessAssociations
            | Operation::RejectDomainNameAccessAssociation
            | Operation::CreateDocumentationPart
            | Operation::GetDocumentationPart
            | Operation::GetDocumentationParts
            | Operation::UpdateDocumentationPart
            | Operation::DeleteDocumentationPart
            | Operation::ImportDocumentationParts
            | Operation::CreateDocumentationVersion
            | Operation::GetDocumentationVersion
            | Operation::GetDocumentationVersions
            | Operation::UpdateDocumentationVersion
            | Operation::DeleteDocumentationVersion
            | Operation::GetSdk
            | Operation::GetSdkType
            | Operation::GetSdkTypes
            | Operation::GetUsage
            | Operation::UpdateUsage
            | Operation::ImportApiKeys
            | Operation::TestInvokeAuthorizer
            | Operation::TestInvokeMethod
            | Operation::GetModelTemplate
    )
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
