//! What a fan-out runs: the input of each Parallel branch or Map item (`ItemsPath`/`Items`, `ItemSelector`, `ItemBatcher`), how
//! many run at once, what failures a distributed Map tolerates, its Map Run's ARN and its `ItemReader` call.

use serde_json::Value;

use super::Exec;
use super::io::{View, text};
use super::run::number;
use crate::def::{Field, Kind, Lang, MapSpec, State};
use crate::env::Env;
use crate::errors::{Failure, RUNTIME};

/// What a fan-out runs: one input per branch or item, and how many at once.
pub(crate) struct Plan {
    pub(crate) inputs: Vec<(Value, Option<Value>)>,
    pub(crate) limit: u32,
    pub(crate) tolerated: Option<(f64, f64)>,
}

pub(crate) fn map_run_arn(ex: &Exec, state: &State, m: &MapSpec, env: &Env) -> String {
    let label = m.label.clone().unwrap_or_else(|| state.name.clone());
    let exec = ex.arn.rsplit(':').next().unwrap_or_default();
    format!(
        "arn:aws:states:{}:{}:mapRun:{}/{label}:{}",
        env.region,
        env.account,
        ex.machine_name,
        env.uuid()
            .get(..8)
            .map_or_else(|| exec.to_owned(), |u| format!("{u}-{exec}"))
    )
}

/// The parameters of an `ItemReader` call, evaluated.
pub(crate) fn read_items(v: &View<'_>, raw: &Value, reader: &Value) -> Result<Value, Failure> {
    let mut out = reader.clone();
    if let Some(p) = reader.get("Parameters") {
        let params = if v.state.lang == Lang::Jsonata {
            v.jsonata("Parameters", p, &v.states(raw, &[]))?
                .unwrap_or(Value::Null)
        } else {
            v.template(p, &v.effective(raw)?, None)?
        };
        if let Some(m) = out.as_object_mut() {
            m.insert("Parameters".into(), params);
        }
    }
    Ok(out)
}

/// The inputs of a fan-out.
pub(crate) fn plan(v: &View<'_>, raw: &Value, read: Option<Value>) -> Result<Plan, Failure> {
    let jsonata = v.state.lang == Lang::Jsonata;
    let eff = if jsonata {
        raw.clone()
    } else {
        v.effective(raw)?
    };
    match &v.state.kind {
        Kind::Parallel { branches } => {
            let input = match (&v.state.parameters, &v.state.arguments) {
                (_, Some(a)) if jsonata => v
                    .jsonata("Arguments", a, &v.states(raw, &[]))?
                    .unwrap_or(Value::Null),
                (Some(p), _) if !jsonata => v.template(p, &eff, None)?,
                _ => eff,
            };
            Ok(Plan {
                inputs: vec![(input, None); branches.len()],
                limit: 0,
                tolerated: None,
            })
        }
        Kind::Map(m) => map_plan(v, raw, &eff, m, read),
        _ => Err(Failure::runtime("not a fan-out")),
    }
}

fn map_plan(
    v: &View<'_>,
    raw: &Value,
    eff: &Value,
    m: &MapSpec,
    read: Option<Value>,
) -> Result<Plan, Failure> {
    let jsonata = v.state.lang == Lang::Jsonata;
    let items = match (read, &m.items, &m.items_path) {
        (Some(r), ..) => r,
        (None, Some(i), _) if jsonata => v
            .jsonata("Items", i, &v.states(raw, &[]))?
            .unwrap_or(Value::Null),
        (None, _, Field::Text(p)) if !jsonata => super::run::select(v, eff, p, "ItemsPath")?,
        _ => eff.clone(),
    };
    let Value::Array(items) = items else {
        // (memory)
        return Err(v.fail(RUNTIME, "The Map state's items must be an array"));
    };
    let items = batch(items, m.item_batcher.as_ref());
    let mut inputs = Vec::with_capacity(items.len());
    for (i, item) in items.into_iter().enumerate() {
        let input = match &m.item_selector {
            Some(sel) => select_item(v, raw, eff, sel, i, &item)?,
            None => item.clone(),
        };
        inputs.push((input, Some(item)));
    }
    let limit = number(v, eff, &m.max_concurrency, "MaxConcurrency")?.unwrap_or(0.0);
    let tolerated = m.distributed.then(|| -> Result<(f64, f64), Failure> {
        Ok((
            number(v, eff, &m.tolerated_count, "ToleratedFailureCount")?.unwrap_or(0.0),
            number(
                v,
                eff,
                &m.tolerated_percentage,
                "ToleratedFailurePercentage",
            )?
            .unwrap_or(0.0),
        ))
    });
    Ok(Plan {
        inputs,
        limit: limit.max(0.0) as u32,
        tolerated: tolerated.transpose()?,
    })
}

/// `ItemSelector` for item `i`: `$$.Map.Item` is the item and its index.
fn select_item(
    v: &View<'_>,
    raw: &Value,
    eff: &Value,
    sel: &Value,
    i: usize,
    item: &Value,
) -> Result<Value, Failure> {
    let mut frame = v.frame.clone();
    frame.item = Some(text(item));
    frame.index = u32::try_from(i).unwrap_or(u32::MAX);
    let view = View {
        ex: v.ex,
        frame: &frame,
        state: v.state,
        env: v.env,
    };
    if v.state.lang == Lang::Jsonata {
        return Ok(view
            .jsonata("ItemSelector", sel, &view.states(raw, &[]))?
            .unwrap_or(Value::Null));
    }
    view.template(sel, eff, None)
}

/// `ItemBatcher`: items in batches of `MaxItemsPerBatch`, each `{"Items": [...]}` with `BatchInput` when given.
fn batch(items: Vec<Value>, batcher: Option<&Value>) -> Vec<Value> {
    let Some(b) = batcher else { return items };
    let size = b
        .get("MaxItemsPerBatch")
        .and_then(Value::as_u64)
        .unwrap_or(items.len().max(1) as u64);
    let size = usize::try_from(size).unwrap_or(1).max(1);
    items
        .chunks(size)
        .map(|c| {
            let mut m = serde_json::Map::new();
            if let Some(input) = b.get("BatchInput") {
                m.insert("BatchInput".into(), input.clone());
            }
            m.insert("Items".into(), Value::Array(c.to_vec()));
            Value::Object(m)
        })
        .collect()
}
