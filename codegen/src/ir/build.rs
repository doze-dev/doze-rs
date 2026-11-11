//! Walks a service: its operations (including those reached through resources), and for each the
//! shapes it mentions.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, anyhow, bail};

use super::{FaultKind, HttpIr, Ir, OpIr, Protocol, Ts, Ty, TypeDef};
use crate::model::{Model, Ref, Shape, Traits, TraitsExt, local_name};

/// Builds the IR for the one service in `model`.
pub fn build(model: &Model) -> Result<Ir> {
    let (svc_id, svc) = model
        .shapes
        .iter()
        .find_map(|(id, s)| match s {
            Shape::Service(d) => Some((id, d)),
            _ => None,
        })
        .context("no service shape")?;
    let protocols = protocols_of(&svc.traits);
    if protocols.is_empty() {
        bail!("service {svc_id} declares no supported protocol");
    }
    let mut b = Builder {
        model,
        json_wire: protocols.contains(&Protocol::RestJson1),
        rest: protocols.iter().any(|p| p.is_rest()),
        types: BTreeMap::new(),
        by_shape: BTreeMap::new(),
        in_progress: BTreeSet::new(),
    };
    let op_ids = operation_ids(model, svc)?;
    let service_errors: Vec<String> = svc.errors.iter().map(|e| e.target.clone()).collect();
    let mut ops = Vec::new();
    for id in &op_ids {
        ops.push(b.operation(id, &service_errors)?);
    }
    ops.sort_by(|a, c| a.name.cmp(&c.name));
    Ok(Ir {
        types: b.types,
        ops,
    })
}

/// The service's operations and those of every resource below it.
fn operation_ids(model: &Model, svc: &crate::model::ServiceDef) -> Result<BTreeSet<String>> {
    let mut op_ids = BTreeSet::new();
    let direct: Vec<String> = svc.operations.iter().map(|r| r.target.clone()).collect();
    collect_ops(model, &direct, &mut op_ids)?;
    let mut resources: Vec<String> = svc.resources.iter().map(|r| r.target.clone()).collect();
    while let Some(rid) = resources.pop() {
        let Some(Shape::Resource(r)) = model.shapes.get(&rid) else {
            bail!("{rid} is not a resource")
        };
        let mut direct: Vec<String> = r
            .operations
            .iter()
            .chain(&r.collection_operations)
            .map(|x| x.target.clone())
            .collect();
        direct.extend(
            [&r.create, &r.put, &r.read, &r.update, &r.delete, &r.list]
                .into_iter()
                .flatten()
                .map(|x| x.target.clone()),
        );
        collect_ops(model, &direct, &mut op_ids)?;
        resources.extend(r.resources.iter().map(|x| x.target.clone()));
    }
    Ok(op_ids)
}

fn collect_ops(model: &Model, ids: &[String], out: &mut BTreeSet<String>) -> Result<()> {
    for id in ids {
        match model.shapes.get(id) {
            Some(Shape::Operation(_)) => {
                out.insert(id.clone());
            }
            other => bail!("{id} is not an operation: {other:?}"),
        }
    }
    Ok(())
}

fn protocols_of(t: &Traits) -> Vec<Protocol> {
    [
        ("aws.protocols#awsJson1_0", Protocol::AwsJson10),
        ("aws.protocols#awsJson1_1", Protocol::AwsJson11),
        ("aws.protocols#awsQuery", Protocol::AwsQuery),
        ("aws.protocols#restJson1", Protocol::RestJson1),
        ("aws.protocols#restXml", Protocol::RestXml),
        ("smithy.protocols#rpcv2Cbor", Protocol::Rpcv2Cbor),
    ]
    .into_iter()
    .filter(|(id, _)| t.contains_key(*id))
    .map(|(_, p)| p)
    .collect()
}

pub(super) fn ts_of(t: &Traits) -> Option<Ts> {
    match t.str_trait("timestampFormat")? {
        "epoch-seconds" => Some(Ts::EpochSeconds),
        "date-time" => Some(Ts::DateTime),
        "http-date" => Some(Ts::HttpDate),
        _ => None,
    }
}

/// The state of one walk. The methods are spread over `shapes` (types) and `members` (structures,
/// unions and member metadata).
pub(super) struct Builder<'m> {
    pub(super) model: &'m Model,
    pub(super) json_wire: bool,
    pub(super) rest: bool,
    pub(super) types: BTreeMap<String, TypeDef>,
    pub(super) by_shape: BTreeMap<String, String>,
    pub(super) in_progress: BTreeSet<String>,
}

impl<'m> Builder<'m> {
    pub(super) fn shape(&self, id: &str) -> Result<&'m Shape> {
        self.model
            .shapes
            .get(id)
            .ok_or_else(|| anyhow!("dangling shape reference {id}"))
    }

    fn operation(&mut self, id: &str, service_errors: &[String]) -> Result<OpIr> {
        let Shape::Operation(op) = self.shape(id)? else {
            bail!("{id} is not an operation")
        };
        let input = self.io(&op.input)?;
        let output = self.io(&op.output)?;
        let mut errors = Vec::new();
        for e in op.errors.iter().map(|e| &e.target).chain(service_errors) {
            if let Ty::Named(n) = self.ty_of_shape(e)?
                && !errors.contains(&n)
            {
                errors.push(n);
            }
        }
        let http = op.traits.get_trait("http").map(http_of).transpose()?;
        Ok(OpIr {
            name: local_name(id).to_owned(),
            input,
            output,
            errors,
            http,
        })
    }

    fn io(&mut self, r: &Option<Ref>) -> Result<Option<String>> {
        match r {
            Some(r) if r.target != "smithy.api#Unit" => match self.ty_of_shape(&r.target)? {
                Ty::Named(n) => Ok(Some(n)),
                other => bail!("operation io {} resolved to {other:?}", r.target),
            },
            _ => Ok(None),
        }
    }

    pub(super) fn error_def(&self, id: &str, t: &Traits) -> Result<Option<super::ErrorDef>> {
        let Some(kind) = t.str_trait("error") else {
            return Ok(None);
        };
        let fault = match kind {
            "client" => FaultKind::Client,
            "server" => FaultKind::Server,
            other => bail!("{id}: unknown error kind {other}"),
        };
        let default_status = if fault == FaultKind::Client { 400 } else { 500 };
        let http_status = t
            .get_trait("httpError")
            .and_then(serde_json::Value::as_u64)
            .map_or(default_status, |c| {
                u16::try_from(c).unwrap_or(default_status)
            });
        let query_code = t
            .get_trait("aws.protocols#awsQueryError")
            .and_then(|v| v.get("code"))
            .and_then(|v| v.as_str())
            .map_or_else(|| local_name(id).to_owned(), str::to_owned);
        Ok(Some(super::ErrorDef {
            query_code,
            http_status,
            fault,
        }))
    }
}

fn http_of(h: &serde_json::Value) -> Result<HttpIr> {
    let text = |key: &str| -> Result<String> {
        Ok(h.get(key)
            .and_then(|v| v.as_str())
            .with_context(|| format!("http.{key}"))?
            .to_owned())
    };
    Ok(HttpIr {
        method: text("method")?,
        uri: text("uri")?,
        code: h
            .get("code")
            .and_then(|v| v.as_u64())
            .map_or(200, |c| u16::try_from(c).unwrap_or(200)),
    })
}
