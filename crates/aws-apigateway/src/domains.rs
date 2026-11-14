//! Custom domain names, shared by both APIs as on AWS (one name, seen by v1 as a `DomainName` with base path mappings and by v2 with
//! API mappings). Stored and returned whole, **not active locally** (SCOPE.md: accept and flag): there is no DNS or TLS here, so
//! nothing is served at the name; an API is reached at its execute-api address. The generated host names and zone ids are
//! `(memory)`-shaped placeholders.

use aws_model_api_gateway::Operation as V1;
use aws_model_apigatewayv2::Operation as V2;
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::doc::{Doc, child, copy, obj, put, text};
use crate::errors::{bad, conflict, not_found};
use crate::patch::{Field, Kind, apply, f};
use crate::req::{Req, Res, new_id, none};

fn no_domain() -> ApiError {
    not_found("Invalid domain name identifier specified")
}

fn no_mapping() -> ApiError {
    not_found("Invalid base path mapping identifier specified")
}

fn create(core: &mut Core, r: &Req, name: &str, v1: Value, v2: Value) -> Result<(), ApiError> {
    if core.domains.get(name).is_some() {
        return Err(conflict("The domain name you provided already exists."));
    }
    tracing::info!("domain name {name} is stored, not active locally: nothing is served at it");
    let regional = format!("d-{}.execute-api.{}.amazonaws.com", new_id(10), r.region());
    let edge = format!("d{}.cloudfront.net", new_id(13));
    let doc = json!({
        "name": name, "v1": v1, "v2": v2, "mappings": {},
        "tags": r.value("tags").cloned().unwrap_or_else(|| json!({})),
        "_regional": regional, "_edge": edge, "_created": r.now_secs(),
    });
    core.domains.insert(name.to_owned(), Doc(doc));
    Ok(())
}

fn arn(r: &Req, name: &str) -> String {
    format!("arn:aws:apigateway:{}::/domainnames/{name}", r.region())
}

/// The domain as v1 shows it.
fn v1_view(d: &Value, r: &Req) -> Value {
    let mut v = obj(d, "v1").cloned().unwrap_or_default();
    let name = text(d, "name").unwrap_or_default();
    v.insert("domainName".into(), json!(name));
    v.insert("domainNameArn".into(), json!(arn(r, name)));
    v.insert("domainNameStatus".into(), json!("AVAILABLE"));
    v.entry("securityPolicy")
        .or_insert_with(|| json!("TLS_1_2"));
    let edge = v
        .get("endpointConfiguration")
        .and_then(|e| e.get("types"))
        .and_then(Value::as_array)
        .is_none_or(|t| t.iter().any(|x| x.as_str() == Some("EDGE")));
    if edge {
        v.insert("distributionDomainName".into(), json!(text(d, "_edge")));
        v.insert("distributionHostedZoneId".into(), json!("Z2FDTNDATAQYW2"));
        v.entry("endpointConfiguration")
            .or_insert_with(|| json!({"types": ["EDGE"], "ipAddressType": "ipv4"}));
    } else {
        v.insert("regionalDomainName".into(), json!(text(d, "_regional")));
        v.insert("regionalHostedZoneId".into(), json!("Z1UJRXOUMOOFQ8"));
    }
    if let Some(at) = d
        .get("_created")
        .filter(|_| v.contains_key("certificateArn"))
    {
        v.insert("certificateUploadDate".into(), at.clone());
    }
    v.insert(
        "tags".into(),
        d.get("tags").cloned().unwrap_or_else(|| json!({})),
    );
    Value::Object(v)
}

/// The domain as v2 shows it.
fn v2_view(d: &Value, r: &Req) -> Value {
    let name = text(d, "name").unwrap_or_default();
    let configs: Vec<Value> = d
        .pointer("/v2/domainNameConfigurations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| vec![json!({})])
        .into_iter()
        .map(|mut c| {
            put(&mut c, "apiGatewayDomainName", json!(text(d, "_regional")));
            put(&mut c, "domainNameStatus", json!("AVAILABLE"));
            put(&mut c, "hostedZoneId", json!("Z1UJRXOUMOOFQ8"));
            for (k, dflt) in [
                ("endpointType", "REGIONAL"),
                ("securityPolicy", "TLS_1_2"),
                ("ipAddressType", "ipv4"),
            ] {
                if c.get(k).is_none() {
                    put(&mut c, k, json!(dflt));
                }
            }
            c
        })
        .collect();
    json!({
        "domainName": name,
        "domainNameArn": arn(r, name),
        "apiMappingSelectionExpression": "$request.basepath",
        "domainNameConfigurations": configs,
        "routingMode": d.pointer("/v2/routingMode").cloned().unwrap_or_else(|| json!("API_MAPPING_ONLY")),
        "tags": d.get("tags").cloned().unwrap_or_else(|| json!({})),
    })
}

/// A v1 domain-name or base-path-mapping operation.
pub(crate) fn v1(core: &mut Core, op: V1, r: &Req) -> Res {
    let name = r.s("domainName");
    match op {
        V1::CreateDomainName => {
            let mut v = Map::new();
            copy(
                &mut v,
                &r.body,
                &[
                    "certificateName",
                    "certificateArn",
                    "regionalCertificateName",
                    "regionalCertificateArn",
                    "endpointConfiguration",
                    "securityPolicy",
                    "mutualTlsAuthentication",
                    "ownershipVerificationCertificateArn",
                    "policy",
                    "routingMode",
                    "endpointAccessMode",
                ],
            );
            create(core, r, name, Value::Object(v), json!({}))?;
            v1_get(core, r, name)
        }
        V1::GetDomainName => v1_get(core, r, name),
        V1::GetDomainNames => {
            let items = core.domains.iter().map(|(_, d)| v1_view(&d.0, r)).collect();
            Ok(crate::v1::page(items, r).into())
        }
        V1::UpdateDomainName => {
            let d = core.domains.get_mut(name).ok_or_else(no_domain)?;
            let mut v = d.0.get("v1").cloned().unwrap_or_else(|| json!({}));
            apply(&mut v, &r.patches(), DOMAIN_FIELDS)?;
            put(&mut d.0, "v1", v);
            v1_get(core, r, name)
        }
        V1::DeleteDomainName => core
            .domains
            .remove(name)
            .map(|_| Default::default())
            .ok_or_else(no_domain),
        _ => mappings_v1(core, op, r, name),
    }
}

const DOMAIN_FIELDS: &[Field] = &[
    f("/certificateName", Kind::Str),
    f("/certificateArn", Kind::Str),
    f("/regionalCertificateName", Kind::Str),
    f("/regionalCertificateArn", Kind::Str),
    f("/securityPolicy", Kind::Str),
    f("/endpointConfiguration/types", Kind::List),
    f("/mutualTlsAuthentication/truststoreUri", Kind::Str),
    f("/mutualTlsAuthentication/truststoreVersion", Kind::Str),
    f("/ownershipVerificationCertificateArn", Kind::Str),
    f("/policy", Kind::Str),
    f("/routingMode", Kind::Str),
];

fn v1_get(core: &Core, r: &Req, name: &str) -> Res {
    core.domains
        .get(name)
        .map(|d| v1_view(&d.0, r).into())
        .ok_or_else(no_domain)
}

fn mapping_v1(m: &Value) -> Value {
    let key = text(m, "key").filter(|k| !k.is_empty()).unwrap_or("(none)");
    json!({"basePath": key, "restApiId": text(m, "apiId"), "stage": text(m, "stage")})
}

fn mappings_v1(core: &mut Core, op: V1, r: &Req, name: &str) -> Res {
    let d = &mut core.domains.get_mut(name).ok_or_else(no_domain)?.0;
    let base = r
        .opt("basePath")
        .map(|b| if b == "(none)" { "" } else { b })
        .unwrap_or_default()
        .to_owned();
    let find = |d: &Value| {
        obj(d, "mappings").and_then(|m| {
            m.iter()
                .find(|(_, x)| text(x, "key") == Some(base.as_str()))
                .map(|(k, _)| k.clone())
        })
    };
    match op {
        V1::CreateBasePathMapping => {
            if find(d).is_some() {
                return Err(conflict("Base path already exists for this domain name"));
            }
            let m =
                json!({"key": base, "apiId": r.s("restApiId"), "stage": r.opt("stage"), "v": 1});
            let shown = mapping_v1(&m);
            if let Some(ms) = child(d, "mappings") {
                ms.insert(new_id(6), m);
            }
            Ok(shown.into())
        }
        V1::GetBasePathMapping => {
            let id = find(d).ok_or_else(no_mapping)?;
            Ok(mapping_v1(
                d.pointer(&format!("/mappings/{id}"))
                    .unwrap_or(&Value::Null),
            )
            .into())
        }
        V1::GetBasePathMappings => {
            let items = obj(d, "mappings")
                .into_iter()
                .flat_map(|m| m.values())
                .map(mapping_v1)
                .collect();
            Ok(crate::v1::page(items, r).into())
        }
        V1::UpdateBasePathMapping => {
            let id = find(d).ok_or_else(no_mapping)?;
            let m = d
                .pointer_mut(&format!("/mappings/{id}"))
                .ok_or_else(no_mapping)?;
            patch_mapping(m, r)?;
            Ok(mapping_v1(m).into())
        }
        V1::DeleteBasePathMapping => {
            let id = find(d).ok_or_else(no_mapping)?;
            child(d, "mappings").and_then(|m| m.remove(&id));
            none()
        }
        _ => Err(crate::errors::refused(&format!("{op:?}"))),
    }
}

/// `UpdateBasePathMapping`'s patches (`/basePath`, `/restapiId`, `/stage`) on a mapping record.
fn patch_mapping(m: &mut Value, r: &Req) -> Result<(), ApiError> {
    let patches: Vec<_> = r
        .patches()
        .into_iter()
        .map(|p| crate::patch::Patch {
            path: match p.path.as_str() {
                "/basePath" => "/key".into(),
                "/restapiId" | "/restApiId" => "/apiId".into(),
                other => other.into(),
            },
            ..p
        })
        .collect();
    let fields = [
        f("/key", Kind::Str),
        f("/apiId", Kind::Str),
        f("/stage", Kind::Str),
    ];
    apply(m, &patches, &fields)
}

/// A v2 domain-name or API-mapping operation.
pub(crate) fn v2(core: &mut Core, op: V2, r: &Req) -> Res {
    let name = r
        .opt("domainName")
        .unwrap_or_else(|| r.s("DomainName"))
        .to_owned();
    match op {
        V2::CreateDomainName => {
            let mut v = Map::new();
            copy(
                &mut v,
                &r.body,
                &[
                    "domainNameConfigurations",
                    "mutualTlsAuthentication",
                    "routingMode",
                ],
            );
            create(core, r, &name, json!({}), Value::Object(v))?;
            v2_get(core, r, &name)
        }
        V2::GetDomainName => v2_get(core, r, &name),
        V2::GetDomainNames => {
            let items: Vec<Value> = core.domains.iter().map(|(_, d)| v2_view(&d.0, r)).collect();
            Ok(json!({"items": items}).into())
        }
        V2::UpdateDomainName => {
            let d = core.domains.get_mut(&name).ok_or_else(no_domain)?;
            for k in [
                "domainNameConfigurations",
                "mutualTlsAuthentication",
                "routingMode",
            ] {
                if let (Some(v), Some(v2)) = (r.value(k), child(&mut d.0, "v2")) {
                    v2.insert(k.to_owned(), v.clone());
                }
            }
            v2_get(core, r, &name)
        }
        V2::DeleteDomainName => core
            .domains
            .remove(&name)
            .map(|_| Default::default())
            .ok_or_else(no_domain),
        _ => mappings_v2(core, op, r, &name),
    }
}

fn v2_get(core: &Core, r: &Req, name: &str) -> Res {
    core.domains
        .get(name)
        .map(|d| v2_view(&d.0, r).into())
        .ok_or_else(|| {
            crate::errors::not_found(format!("Invalid domain name identifier specified: {name}"))
        })
}

fn mapping_v2(id: &str, m: &Value) -> Value {
    json!({"apiId": text(m, "apiId"), "apiMappingId": id, "apiMappingKey": text(m, "key").unwrap_or_default(), "stage": text(m, "stage")})
}

fn mappings_v2(core: &mut Core, op: V2, r: &Req, name: &str) -> Res {
    let d = &mut core.domains.get_mut(name).ok_or_else(no_domain)?.0;
    let id = r.s("ApiMappingId").to_owned();
    let missing = || not_found(format!("Invalid api mapping identifier specified {id}"));
    match op {
        V2::CreateApiMapping => {
            let key = r.s("apiMappingKey");
            let taken = obj(d, "mappings")
                .into_iter()
                .flat_map(|m| m.values())
                .any(|m| text(m, "key") == Some(key));
            if taken {
                return Err(conflict(format!(
                    "ApiMapping key {key} already exists for domain name {name}"
                )));
            }
            let mid = new_id(6);
            let m = json!({"key": key, "apiId": r.s("apiId"), "stage": r.s("stage"), "v": 2});
            let shown = mapping_v2(&mid, &m);
            if let Some(ms) = child(d, "mappings") {
                ms.insert(mid, m);
            }
            Ok(shown.into())
        }
        V2::GetApiMapping => d
            .pointer(&format!("/mappings/{id}"))
            .map(|m| mapping_v2(&id, m).into())
            .ok_or_else(missing),
        V2::GetApiMappings => {
            let items: Vec<Value> = obj(d, "mappings")
                .into_iter()
                .flat_map(|m| m.iter())
                .map(|(k, m)| mapping_v2(k, m))
                .collect();
            Ok(json!({"items": items}).into())
        }
        V2::UpdateApiMapping => {
            let m = d
                .pointer_mut(&format!("/mappings/{id}"))
                .ok_or_else(missing)?;
            for (from, to) in [
                ("apiMappingKey", "key"),
                ("apiId", "apiId"),
                ("stage", "stage"),
            ] {
                if let Some(v) = r.value(from) {
                    put(m, to, v.clone());
                }
            }
            Ok(mapping_v2(&id, m).into())
        }
        V2::DeleteApiMapping => {
            child(d, "mappings")
                .and_then(|m| m.remove(&id))
                .ok_or_else(missing)?;
            none()
        }
        _ => Err(bad(format!("{op:?} is not a domain operation"))),
    }
}
