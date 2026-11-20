//! VPC links of both APIs. There is no VPC locally, so a link is kept as AWS shows it and follows AWS's status lifecycle without
//! the wait: the create answers `PENDING` (the model: links are made "in an asynchronous operation that typically takes 2-4
//! minutes"), and every read after it `AVAILABLE`. An integration with `connectionType: VPC_LINK` calls its integration URI
//! directly (a REST API's URI is the NLB's address, so a local address works as it is). v1 links are `v1/<id>` in the `links`
//! table and v2 links `v2/<id>`; the two APIs do not see each other's links, as on AWS. The status messages and refusal words are
//! `(memory)`.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::doc::{Doc, copy, put, with_tags};
use crate::errors::{bad, not_found};
use crate::patch::{Kind, apply, f};
use crate::req::{Req, Res, new_id, none};

fn no_link(id: &str) -> ApiError {
    not_found(format!("Invalid VPC link identifier specified {id}"))
}

/// The stored record shown as read: `AVAILABLE`.
fn settled(mut v: Value, v2: bool) -> Value {
    if v2 {
        put(&mut v, "vpcLinkStatus", json!("AVAILABLE"));
        put(
            &mut v,
            "vpcLinkStatusMessage",
            json!("VPC link is ready to route traffic"),
        );
    } else {
        put(&mut v, "status", json!("AVAILABLE"));
        put(
            &mut v,
            "statusMessage",
            json!("Your vpc link is ready for use"),
        );
    }
    v
}

/// `CreateVpcLink` (v1): one network load balancer ARN in `targetArns` (memory: AWS takes one).
pub(crate) fn create_v1(core: &mut Core, r: &Req) -> Res {
    let targets: Vec<&str> = r
        .value("targetArns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    match targets.as_slice() {
        [one] if one.starts_with("arn:aws:elasticloadbalancing:") => {}
        [_] => {
            return Err(bad(
                "Invalid target ARN: a VPC link targets a network load balancer (arn:aws:elasticloadbalancing:...)",
            ));
        }
        _ => return Err(bad("A VPC link takes exactly one target ARN")),
    }
    let id = new_id(6);
    let mut v = Map::new();
    v.insert("id".into(), json!(id));
    copy(
        &mut v,
        &r.body,
        &["name", "description", "targetArns", "tags"],
    );
    let v = Value::Object(v);
    core.links
        .insert(format!("v1/{id}"), Doc(settled(v.clone(), false)));
    let mut shown = v;
    put(&mut shown, "status", json!("PENDING"));
    put(
        &mut shown,
        "statusMessage",
        json!("Your vpc link is being created"),
    );
    Ok(shown.into())
}

/// The v1 link operations other than create.
pub(crate) fn v1(core: &mut Core, op: &str, r: &Req) -> Res {
    let id = r.s("vpcLinkId");
    let key = format!("v1/{id}");
    match op {
        "GetVpcLink" => core
            .links
            .get(&key)
            .map(|d| with_tags(d.0.clone()).into())
            .ok_or_else(|| no_link(id)),
        "GetVpcLinks" => {
            let items = core
                .links
                .with_prefix("v1/")
                .map(|(_, d)| with_tags(d.0.clone()))
                .collect();
            Ok(crate::v1::page(items, r).into())
        }
        "UpdateVpcLink" => {
            let slot = core.links.get_mut(&key).ok_or_else(|| no_link(id))?;
            let mut v = slot.0.clone();
            apply(
                &mut v,
                &r.patches(),
                &[f("/name", Kind::Str), f("/description", Kind::Str)],
            )?;
            slot.0 = v.clone();
            Ok(with_tags(v).into())
        }
        _ => core
            .links
            .remove(&key)
            .ok_or_else(|| no_link(id))
            .and_then(|_| none()),
    }
}

/// The v2 link operations.
pub(crate) fn v2(core: &mut Core, op: &str, r: &Req) -> Res {
    let id = r.s("VpcLinkId");
    let key = format!("v2/{id}");
    match op {
        "CreateVpcLink" => create_v2(core, r),
        "GetVpcLink" => core
            .links
            .get(&key)
            .map(|d| d.0.clone().into())
            .ok_or_else(|| no_link(id)),
        "GetVpcLinks" => {
            let items = core
                .links
                .with_prefix("v2/")
                .map(|(_, d)| d.0.clone())
                .collect();
            Ok(crate::v2::page(items, r).into())
        }
        "UpdateVpcLink" => {
            let slot = core.links.get_mut(&key).ok_or_else(|| no_link(id))?;
            if let Some(n) = r.opt("name") {
                put(&mut slot.0, "name", json!(n));
            }
            Ok(slot.0.clone().into())
        }
        _ => core
            .links
            .remove(&key)
            .ok_or_else(|| no_link(id))
            .map(|_| json!({}).into()),
    }
}

fn create_v2(core: &mut Core, r: &Req) -> Res {
    let subnets = r
        .value("subnetIds")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if subnets == 0 {
        return Err(bad("A VPC link needs at least one subnet id"));
    }
    let id = new_id(6);
    let mut v = Map::new();
    v.insert("vpcLinkId".into(), json!(id));
    v.insert("createdDate".into(), json!(r.now_iso()));
    copy(&mut v, &r.body, &["name", "subnetIds"]);
    v.insert(
        "securityGroupIds".into(),
        r.value("securityGroupIds")
            .cloned()
            .unwrap_or_else(|| json!([])),
    );
    v.insert(
        "tags".into(),
        r.value("tags").cloned().unwrap_or_else(|| json!({})),
    );
    v.insert("vpcLinkVersion".into(), json!("V2"));
    let v = Value::Object(v);
    core.links
        .insert(format!("v2/{id}"), Doc(settled(v.clone(), true)));
    let mut shown = v;
    put(&mut shown, "vpcLinkStatus", json!("PENDING"));
    put(
        &mut shown,
        "vpcLinkStatusMessage",
        json!("VPC link is provisioning ENIs"),
    );
    Ok(shown.into())
}

/// A REST integration with `connectionType: VPC_LINK` must name a link that exists in `connectionId` (one named by a stage
/// variable, `${stageVariables.x}`, is not checked).
pub(crate) fn check_v1(core: &Core, r: &Req) -> Result<(), ApiError> {
    if r.opt("connectionType") != Some("VPC_LINK") {
        return Ok(());
    }
    let id = r.s("connectionId");
    if id.is_empty() {
        return Err(bad(
            "ConnectionId must be specified for connectionType VPC_LINK",
        ));
    }
    if id.contains("${") || core.links.get(&format!("v1/{id}")).is_some() {
        Ok(())
    } else {
        Err(no_link(id))
    }
}

/// An HTTP API integration's URI over a VPC link names a load balancer listener or a Cloud Map service by ARN, which cannot be
/// called locally: the plane says so.
pub(crate) fn unreachable(uri: &str) -> Option<String> {
    uri.starts_with("arn:").then(|| {
        format!(
            "the integration reaches {uri} over a VPC link; there is no VPC locally, so give the integration an http(s) URI to call directly"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::text;

    #[test]
    fn a_read_link_is_available() {
        let v = settled(json!({"id": "a"}), false);
        assert_eq!(text(&v, "status"), Some("AVAILABLE"));
        assert!(unreachable("arn:aws:elasticloadbalancing:us-east-1:1:listener/app/x").is_some());
        assert!(unreachable("http://localhost:9000").is_none());
    }
}
