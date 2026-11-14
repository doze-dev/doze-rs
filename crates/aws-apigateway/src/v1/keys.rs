//! API keys, usage plans and the keys in them, the account settings, custom domain names (stored, nothing served at them), and tags.
//! A usage plan's keys are kept on the plan (`_keys`), so a key in a plan and the plan-key views come from one list. Throttle and
//! quota are stored and returned, never enforced: nothing is metered locally.

use aws_model_api_gateway::Operation;
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use super::page;
use crate::core::Core;
use crate::doc::{Doc, copy, public, put, text};
use crate::errors::{bad, conflict, not_found};
use crate::patch::{Field, Kind, Patch, apply, f};
use crate::req::{Req, Res, new_id, none};

pub(super) fn dispatch(core: &mut Core, op: Operation, r: &Req) -> Option<Res> {
    Some(match op {
        Operation::CreateApiKey => create_key(core, r),
        Operation::GetApiKey => key(core, r.s("apiKey"))
            .map(|k| key_view(k, r.flag("includeValue") == Some(true)).into()),
        Operation::GetApiKeys => Ok(list_keys(core, r).into()),
        Operation::UpdateApiKey => update_key(core, r),
        Operation::DeleteApiKey => delete_key(core, r),
        Operation::CreateUsagePlan => create_plan(core, r),
        Operation::GetUsagePlan => plan(core, r.s("usagePlanId")).map(|p| public(p).into()),
        Operation::GetUsagePlans => Ok(list_plans(core, r).into()),
        Operation::UpdateUsagePlan => update_plan(core, r),
        Operation::DeleteUsagePlan => core
            .plans
            .remove(r.s("usagePlanId"))
            .map(|_| Default::default())
            .ok_or_else(no_plan),
        Operation::CreateUsagePlanKey => create_plan_key(core, r),
        Operation::GetUsagePlanKey => plan_key(core, r, r.s("keyId")).map(Into::into),
        Operation::GetUsagePlanKeys => plan_keys(core, r),
        Operation::DeleteUsagePlanKey => delete_plan_key(core, r),
        Operation::GetAccount => Ok(account(core).into()),
        Operation::UpdateAccount => update_account(core, r),
        Operation::CreateDomainName
        | Operation::GetDomainName
        | Operation::GetDomainNames
        | Operation::UpdateDomainName
        | Operation::DeleteDomainName
        | Operation::CreateBasePathMapping
        | Operation::GetBasePathMapping
        | Operation::GetBasePathMappings
        | Operation::UpdateBasePathMapping
        | Operation::DeleteBasePathMapping => crate::domains::v1(core, op, r),
        Operation::GetTags => {
            crate::tags::get(core, r.s("resourceArn")).map(|t| json!({"tags": t}).into())
        }
        Operation::TagResource => {
            crate::tags::tag(core, r.s("resourceArn"), r.value("tags")).and_then(|()| none())
        }
        Operation::UntagResource => {
            crate::tags::untag(core, r.s("resourceArn"), r.value("tagKeys")).and_then(|()| none())
        }
        _ => return None,
    })
}

fn no_key() -> ApiError {
    not_found("Invalid API Key identifier specified")
}

fn no_plan() -> ApiError {
    not_found("Invalid Usage Plan ID specified")
}

fn key<'a>(core: &'a Core, id: &str) -> Result<&'a Value, ApiError> {
    core.keys.get(id).map(|d| &d.0).ok_or_else(no_key)
}

fn plan<'a>(core: &'a Core, id: &str) -> Result<&'a Value, ApiError> {
    core.plans.get(id).map(|d| &d.0).ok_or_else(no_plan)
}

fn key_view(k: &Value, with_value: bool) -> Value {
    let mut v = public(k);
    if !with_value && let Some(o) = v.as_object_mut() {
        o.remove("value");
    }
    v
}

fn create_key(core: &mut Core, r: &Req) -> Res {
    let value = r.opt("value").map_or_else(|| new_id(40), str::to_owned);
    if value.len() < 20 {
        return Err(bad("API Key value should be at least 20 characters"));
    }
    if core
        .keys
        .iter()
        .any(|(_, k)| text(&k.0, "value") == Some(&value))
    {
        return Err(conflict("API Key already exists"));
    }
    let id = new_id(10);
    let mut k = Map::new();
    k.insert("id".into(), json!(id));
    k.insert("value".into(), json!(value));
    copy(
        &mut k,
        &r.body,
        &["name", "customerId", "description", "tags"],
    );
    k.insert("enabled".into(), json!(r.flag("enabled").unwrap_or(false)));
    k.insert("createdDate".into(), json!(r.now_secs()));
    k.insert("lastUpdatedDate".into(), json!(r.now_secs()));
    let stages: Vec<Value> = r
        .value("stageKeys")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|s| {
            json!(format!(
                "{}/{}",
                text(s, "restApiId").unwrap_or_default(),
                text(s, "stageName").unwrap_or_default()
            ))
        })
        .collect();
    k.insert("stageKeys".into(), Value::Array(stages));
    let k = Value::Object(k);
    core.keys.insert(id, Doc(k.clone()));
    Ok(k.into())
}

fn list_keys(core: &Core, r: &Req) -> Value {
    let with = r.flag("includeValues") == Some(true);
    let items = core
        .keys
        .iter()
        .map(|(_, k)| &k.0)
        .filter(|k| {
            r.opt("nameQuery")
                .is_none_or(|q| text(k, "name").is_some_and(|n| n.starts_with(q)))
        })
        .filter(|k| {
            r.opt("customerId")
                .is_none_or(|c| text(k, "customerId") == Some(c))
        })
        .map(|k| key_view(k, with))
        .collect();
    page(items, r)
}

const KEY_FIELDS: &[Field] = &[
    f("/name", Kind::Str),
    f("/description", Kind::Str),
    f("/enabled", Kind::Bool),
    f("/customerId", Kind::Str),
    f("/stages", Kind::List),
];

fn update_key(core: &mut Core, r: &Req) -> Res {
    let k = core.keys.get_mut(r.s("apiKey")).ok_or_else(no_key)?;
    let mut v = k.0.clone();
    // `/stages` is what the API calls the key's stage list; it is kept as `stageKeys`.
    let patches: Vec<Patch> = r
        .patches()
        .into_iter()
        .map(|p| Patch {
            path: p.path.replacen("/stages", "/stageKeys", 1),
            ..p
        })
        .collect();
    let fields: Vec<Field> = KEY_FIELDS
        .iter()
        .map(|x| {
            if x.path == "/stages" {
                f("/stageKeys", Kind::List)
            } else {
                *x
            }
        })
        .collect();
    apply(&mut v, &patches, &fields)?;
    put(&mut v, "lastUpdatedDate", json!(r.now_secs()));
    k.0 = v;
    Ok(key_view(&k.0, false).into())
}

fn delete_key(core: &mut Core, r: &Req) -> Res {
    let id = r.s("apiKey");
    core.keys.remove(id).ok_or_else(no_key)?;
    let plans: Vec<String> = core.plans.iter().map(|(k, _)| k.to_owned()).collect();
    for p in plans {
        if let Some(Doc(plan)) = core.plans.get_mut(&p)
            && let Some(keys) = plan.get_mut("_keys").and_then(Value::as_array_mut)
        {
            keys.retain(|k| k.as_str() != Some(id));
        }
    }
    none()
}

/// The `{apiId, stage}` entries of a plan must name a stage that exists `(memory)` for the words.
fn check_stages(core: &Core, stages: Option<&Value>) -> Result<(), ApiError> {
    for s in stages.and_then(Value::as_array).into_iter().flatten() {
        let (api, stage) = (
            text(s, "apiId").unwrap_or_default(),
            text(s, "stage").unwrap_or_default(),
        );
        let found = core
            .rest
            .get(api)
            .and_then(|d| d.0.pointer(&format!("/stages/{stage}")))
            .is_some();
        if !found {
            return Err(bad(format!("API Stage not found: {api}:{stage}")));
        }
    }
    Ok(())
}

fn create_plan(core: &mut Core, r: &Req) -> Res {
    check_stages(core, r.value("apiStages"))?;
    let id = new_id(6);
    let mut p = Map::new();
    p.insert("id".into(), json!(id));
    copy(
        &mut p,
        &r.body,
        &["name", "description", "throttle", "quota", "tags"],
    );
    p.insert(
        "apiStages".into(),
        r.value("apiStages").cloned().unwrap_or_else(|| json!([])),
    );
    let p = Value::Object(p);
    core.plans.insert(id, Doc(p.clone()));
    Ok(p.into())
}

fn list_plans(core: &Core, r: &Req) -> Value {
    let items = core
        .plans
        .iter()
        .map(|(_, p)| &p.0)
        .filter(|p| {
            r.opt("keyId").is_none_or(|k| {
                p.get("_keys")
                    .and_then(Value::as_array)
                    .is_some_and(|ks| ks.iter().any(|x| x.as_str() == Some(k)))
            })
        })
        .map(public)
        .collect();
    page(items, r)
}

const PLAN_FIELDS: &[Field] = &[
    f("/name", Kind::Str),
    f("/description", Kind::Str),
    f("/productCode", Kind::Str),
    f("/throttle/burstLimit", Kind::Int),
    f("/throttle/rateLimit", Kind::Num),
    f("/quota/limit", Kind::Int),
    f("/quota/offset", Kind::Int),
    f("/quota/period", Kind::Str),
    f("/throttle", Kind::Str),
    f("/quota", Kind::Str),
];

fn update_plan(core: &mut Core, r: &Req) -> Res {
    let id = r.s("usagePlanId");
    let mut v = plan(core, id)?.clone();
    let (stages, rest): (Vec<Patch>, Vec<Patch>) = r
        .patches()
        .into_iter()
        .partition(|p| p.path == "/apiStages");
    apply(&mut v, &rest, PLAN_FIELDS)?;
    for p in &stages {
        let (api, stage) = p
            .value
            .as_deref()
            .and_then(|s| s.split_once(':'))
            .ok_or_else(|| bad("Invalid API Stage specified: expected apiId:stage"))?;
        let entry = json!({"apiId": api, "stage": stage});
        if p.op == "add" {
            check_stages(core, Some(&json!([entry])))?;
        }
        let stages = v
            .as_object_mut()
            .map(|o| o.entry("apiStages").or_insert_with(|| json!([])));
        if let Some(Value::Array(list)) = stages {
            list.retain(|s| !(text(s, "apiId") == Some(api) && text(s, "stage") == Some(stage)));
            if p.op == "add" {
                list.push(entry);
            }
        }
    }
    let shown = public(&v);
    core.plans.insert(id.to_owned(), Doc(v));
    Ok(shown.into())
}

fn create_plan_key(core: &mut Core, r: &Req) -> Res {
    let key_id = r.s("keyId");
    let view = plan_key_view(key(core, key_id)?);
    let p = core.plans.get_mut(r.s("usagePlanId")).ok_or_else(no_plan)?;
    let keys =
        p.0.as_object_mut()
            .map(|o| o.entry("_keys").or_insert_with(|| json!([])));
    if let Some(Value::Array(keys)) = keys {
        if keys.iter().any(|k| k.as_str() == Some(key_id)) {
            return Err(conflict(format!(
                "Usage plan already contains API key {key_id}"
            )));
        }
        keys.push(json!(key_id));
    }
    Ok(view.into())
}

fn plan_key_view(k: &Value) -> Value {
    let mut v = json!({"id": text(k, "id"), "type": "API_KEY", "value": text(k, "value")});
    if let Some(n) = text(k, "name") {
        put(&mut v, "name", json!(n));
    }
    v
}

fn keys_of(core: &Core, plan_id: &str) -> Result<Vec<String>, ApiError> {
    Ok(plan(core, plan_id)?
        .get("_keys")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|k| k.as_str().map(str::to_owned))
        .collect())
}

fn plan_key(core: &Core, r: &Req, key_id: &str) -> Result<Value, ApiError> {
    if !keys_of(core, r.s("usagePlanId"))?
        .iter()
        .any(|k| k == key_id)
    {
        return Err(not_found("Invalid Usage Plan Key identifier specified"));
    }
    key(core, key_id).map(plan_key_view)
}

fn plan_keys(core: &Core, r: &Req) -> Res {
    let items = keys_of(core, r.s("usagePlanId"))?
        .iter()
        .filter_map(|k| core.keys.get(k))
        .map(|k| plan_key_view(&k.0))
        .filter(|k| {
            r.opt("nameQuery")
                .is_none_or(|q| text(k, "name").is_some_and(|n| n.starts_with(q)))
        })
        .collect();
    Ok(page(items, r).into())
}

fn delete_plan_key(core: &mut Core, r: &Req) -> Res {
    plan_key(core, r, r.s("keyId"))?;
    if let Some(p) = core.plans.get_mut(r.s("usagePlanId"))
        && let Some(keys) = p.0.get_mut("_keys").and_then(Value::as_array_mut)
    {
        keys.retain(|k| k.as_str() != Some(r.s("keyId")));
    }
    none()
}

/// The account settings, with the defaults AWS reports `(memory)`.
fn account(core: &Core) -> Value {
    let mut v = json!({
        "throttleSettings": {"burstLimit": 5000, "rateLimit": 10000.0},
        "features": ["UsagePlans"],
        "apiKeyVersion": "4",
    });
    if let Some(saved) = core.account.get("account").and_then(|d| d.0.as_object()) {
        for (k, x) in saved {
            put(&mut v, k, x.clone());
        }
    }
    v
}

fn update_account(core: &mut Core, r: &Req) -> Res {
    let mut v = account(core);
    apply(
        &mut v,
        &r.patches(),
        &[
            f("/cloudwatchRoleArn", Kind::Str),
            f("/features", Kind::List),
        ],
    )?;
    core.account.insert("account".into(), Doc(v.clone()));
    Ok(v.into())
}
