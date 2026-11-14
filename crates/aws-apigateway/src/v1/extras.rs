//! The parts of a REST API beside its resource tree: authorizers, request validators, models and gateway responses. Each is a map in
//! the API document (`authorizers` and `validators` by id, `models` by name, `responses` by type).

use aws_model_api_gateway::Operation;
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use super::{api, api_mut, page};
use crate::core::Core;
use crate::doc::{child, copy, items, obj, put, text};
use crate::errors::{bad, conflict, invalid_id, not_found};
use crate::gwresp;
use crate::patch::{Field, Kind, apply, f};
use crate::req::{Req, Res, new_id, none};

pub(super) fn dispatch(core: &mut Core, op: Operation, r: &Req) -> Option<Res> {
    Some(match op {
        Operation::CreateAuthorizer => create_authorizer(core, r),
        Operation::GetAuthorizer => get(core, r, "authorizers", "authorizerId", "Authorizer"),
        Operation::GetAuthorizers => list(core, r, "authorizers"),
        Operation::UpdateAuthorizer => update(
            core,
            r,
            "authorizers",
            "authorizerId",
            "Authorizer",
            AUTHORIZER_FIELDS,
        ),
        Operation::DeleteAuthorizer => delete_authorizer(core, r),
        Operation::CreateRequestValidator => create_validator(core, r),
        Operation::GetRequestValidator => get(
            core,
            r,
            "validators",
            "requestValidatorId",
            "Request Validator",
        ),
        Operation::GetRequestValidators => list(core, r, "validators"),
        Operation::UpdateRequestValidator => update(
            core,
            r,
            "validators",
            "requestValidatorId",
            "Request Validator",
            VALIDATOR_FIELDS,
        ),
        Operation::DeleteRequestValidator => delete(
            core,
            r,
            "validators",
            "requestValidatorId",
            "Request Validator",
        ),
        Operation::CreateModel => create_model(core, r),
        Operation::GetModel => get(core, r, "models", "modelName", "Model"),
        Operation::GetModels => list(core, r, "models"),
        Operation::UpdateModel => update(core, r, "models", "modelName", "Model", MODEL_FIELDS),
        Operation::DeleteModel => delete(core, r, "models", "modelName", "Model"),
        Operation::PutGatewayResponse => put_response(core, r),
        Operation::GetGatewayResponse => {
            api(core, r).map(|d| gwresp::of(obj(d, "responses"), r.s("responseType")).into())
        }
        Operation::GetGatewayResponses => {
            api(core, r).map(|d| page(gwresp::all(obj(d, "responses")), r).into())
        }
        Operation::UpdateGatewayResponse => update_response(core, r),
        Operation::DeleteGatewayResponse => delete_response(core, r),
        _ => return None,
    })
}

fn get(core: &Core, r: &Req, part: &str, key: &str, thing: &str) -> Res {
    let doc = api(core, r)?;
    obj(doc, part)
        .and_then(|m| m.get(r.s(key)))
        .cloned()
        .map(Into::into)
        .ok_or_else(|| invalid_id(thing))
}

fn list(core: &Core, r: &Req, part: &str) -> Res {
    let doc = api(core, r)?;
    Ok(page(items(obj(doc, part)), r).into())
}

fn update(core: &mut Core, r: &Req, part: &str, key: &str, thing: &str, fields: &[Field]) -> Res {
    let doc = api_mut(core, r)?;
    let id = r.s(key);
    let slot = child(doc, part)
        .and_then(|m| m.get_mut(id))
        .ok_or_else(|| invalid_id(thing))?;
    let mut v = slot.clone();
    apply(&mut v, &r.patches(), fields)?;
    // WHY: the record and the answer are two values.
    *slot = v.clone();
    Ok(v.into())
}

fn delete(core: &mut Core, r: &Req, part: &str, key: &str, thing: &str) -> Res {
    let doc = api_mut(core, r)?;
    let id = r.s(key);
    child(doc, part)
        .and_then(|m| m.remove(id))
        .ok_or_else(|| invalid_id(thing))?;
    none()
}

const AUTHORIZER_FIELDS: &[Field] = &[
    f("/name", Kind::Str),
    f("/type", Kind::Str),
    f("/providerARNs", Kind::List),
    f("/authType", Kind::Str),
    f("/authorizerUri", Kind::Str),
    f("/authorizerCredentials", Kind::Str),
    f("/identitySource", Kind::Str),
    f("/identityValidationExpression", Kind::Str),
    f("/authorizerResultTtlInSeconds", Kind::Int),
];

fn create_authorizer(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let kind = r.s("type");
    let name = r.s("name");
    let taken = obj(doc, "authorizers")
        .into_iter()
        .flat_map(|m| m.values())
        .any(|a| text(a, "name") == Some(name));
    if taken {
        return Err(conflict(format!("Authorizer name already exists: {name}")));
    }
    check_authorizer(kind, r)?;
    let id = new_id(6);
    let mut a = Map::new();
    a.insert("id".into(), json!(id));
    a.insert("name".into(), json!(name));
    a.insert("type".into(), json!(kind));
    copy(
        &mut a,
        &r.body,
        &[
            "providerARNs",
            "authType",
            "authorizerUri",
            "authorizerCredentials",
            "identitySource",
            "identityValidationExpression",
        ],
    );
    if kind == "TOKEN" && !a.contains_key("identitySource") {
        a.insert(
            "identitySource".into(),
            json!("method.request.header.Authorization"),
        );
    }
    a.insert(
        "authorizerResultTtlInSeconds".into(),
        json!(r.int("authorizerResultTtlInSeconds").unwrap_or(300)),
    );
    let a = Value::Object(a);
    if let Some(m) = child(doc, "authorizers") {
        m.insert(id, a.clone());
    }
    Ok(a.into())
}

/// What each kind of authorizer must name `(memory)` for the words.
fn check_authorizer(kind: &str, r: &Req) -> Result<(), ApiError> {
    match kind {
        "TOKEN" | "REQUEST" if r.opt("authorizerUri").is_none_or(str::is_empty) => Err(bad(
            format!("Authorizer uri is required for authorizer type {kind}"),
        )),
        "COGNITO_USER_POOLS"
            if r.value("providerARNs")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty) =>
        {
            Err(bad(
                "ProviderARNs need to be specified for authorizer type COGNITO_USER_POOLS",
            ))
        }
        _ => Ok(()),
    }
}

fn delete_authorizer(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let id = r.s("authorizerId");
    let used = obj(doc, "resources")
        .into_iter()
        .flat_map(|rs| rs.values())
        .flat_map(|res| {
            obj(res, "resourceMethods")
                .into_iter()
                .flat_map(|m| m.values())
        })
        .any(|m| text(m, "authorizerId") == Some(id));
    if used {
        return Err(conflict(format!(
            "Cannot delete authorizer '{id}', is referenced in a method"
        )));
    }
    delete(core, r, "authorizers", "authorizerId", "Authorizer")
}

const VALIDATOR_FIELDS: &[Field] = &[
    f("/name", Kind::Str),
    f("/validateRequestBody", Kind::Bool),
    f("/validateRequestParameters", Kind::Bool),
];

fn create_validator(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let id = new_id(6);
    let mut v = Map::new();
    v.insert("id".into(), json!(id));
    copy(&mut v, &r.body, &["name"]);
    v.insert(
        "validateRequestBody".into(),
        json!(r.flag("validateRequestBody").unwrap_or(false)),
    );
    v.insert(
        "validateRequestParameters".into(),
        json!(r.flag("validateRequestParameters").unwrap_or(false)),
    );
    let v = Value::Object(v);
    if let Some(m) = child(doc, "validators") {
        m.insert(id, v.clone());
    }
    Ok(v.into())
}

const MODEL_FIELDS: &[Field] = &[f("/description", Kind::Str), f("/schema", Kind::Str)];

fn create_model(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let name = r.s("name");
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(bad("Model name must be alphanumeric"));
    }
    if obj(doc, "models").is_some_and(|m| m.contains_key(name)) {
        return Err(conflict("Model name already exists for this REST API"));
    }
    if let Some(schema) = r.opt("schema")
        && serde_json::from_str::<Value>(schema).is_err()
    {
        return Err(bad(
            "Invalid model specified: Validation Result: warnings : [], errors : [Invalid model schema specified]",
        ));
    }
    let model = new_model(
        name,
        r.opt("description"),
        r.opt("schema"),
        r.s("contentType"),
    );
    if let Some(m) = child(doc, "models") {
        m.insert(name.to_owned(), model.clone());
    }
    Ok(model.into())
}

/// A model record.
pub(crate) fn new_model(
    name: &str,
    description: Option<&str>,
    schema: Option<&str>,
    content: &str,
) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(new_id(6)));
    m.insert("name".into(), json!(name));
    if let Some(d) = description {
        m.insert("description".into(), json!(d));
    }
    if let Some(s) = schema {
        m.insert("schema".into(), json!(s));
    }
    m.insert("contentType".into(), json!(content));
    Value::Object(m)
}

fn put_response(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let kind = r.s("responseType");
    let mut v = Map::new();
    v.insert("defaultResponse".into(), json!(false));
    v.insert(
        "responseParameters".into(),
        r.value("responseParameters")
            .cloned()
            .unwrap_or_else(|| json!({})),
    );
    v.insert(
        "responseTemplates".into(),
        r.value("responseTemplates")
            .cloned()
            .unwrap_or_else(|| json!({})),
    );
    v.insert("responseType".into(), json!(kind));
    let status = r.opt("statusCode").map(str::to_owned).or_else(|| {
        gwresp::default_of(kind)
            .get("statusCode")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    if let Some(code) = status {
        v.insert("statusCode".into(), json!(code));
    }
    let v = Value::Object(v);
    if let Some(m) = child(doc, "responses") {
        m.insert(kind.to_owned(), v.clone());
    }
    Ok(v.into())
}

const RESPONSE_FIELDS: &[Field] = &[
    f("/statusCode", Kind::Str),
    f("/responseParameters", Kind::Map),
    f("/responseTemplates", Kind::Map),
];

fn update_response(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let kind = r.s("responseType");
    let mut v = gwresp::of(obj(doc, "responses"), kind);
    apply(&mut v, &r.patches(), RESPONSE_FIELDS)?;
    put(&mut v, "defaultResponse", json!(false));
    if let Some(m) = child(doc, "responses") {
        m.insert(kind.to_owned(), v.clone());
    }
    Ok(v.into())
}

fn delete_response(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let kind = r.s("responseType");
    child(doc, "responses")
        .and_then(|m| m.remove(kind))
        .ok_or_else(|| not_found("Invalid Gateway Response identifier specified"))?;
    none()
}
