//! Methods, their integrations, and the responses of both. A method lives under its resource (`resourceMethods.GET`) in the shape
//! `GetMethod` shows: its integration under `methodIntegration` (with `integrationResponses`), its responses under `methodResponses`.

use aws_model_api_gateway::Operation;
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use super::{api, api_mut};
use crate::doc::{child, copy, obj, put, text};
use crate::errors::{bad, conflict, invalid_id};
use crate::patch::{Field, Kind, apply, f};
use crate::req::{Req, Res, none};

pub(super) fn dispatch(core: &mut crate::core::Core, op: Operation, r: &Req) -> Option<Res> {
    Some(match op {
        Operation::PutMethod => put_method(api_mut(core, r), r),
        Operation::GetMethod => api(core, r).and_then(|d| at(d, r, &[]).cloned().map(Into::into)),
        Operation::UpdateMethod => update(api_mut(core, r), r, &[], METHOD_FIELDS),
        Operation::DeleteMethod => delete(api_mut(core, r), r, &[]),
        Operation::PutIntegration => put_integration(api_mut(core, r), r),
        Operation::GetIntegration => {
            api(core, r).and_then(|d| at(d, r, &["methodIntegration"]).cloned().map(Into::into))
        }
        Operation::UpdateIntegration => update(
            api_mut(core, r),
            r,
            &["methodIntegration"],
            INTEGRATION_FIELDS,
        ),
        Operation::DeleteIntegration => delete(api_mut(core, r), r, &["methodIntegration"]),
        Operation::PutMethodResponse => put_method_response(api_mut(core, r), r),
        Operation::GetMethodResponse => api(core, r).and_then(|d| {
            at(d, r, &["methodResponses", r.s("statusCode")])
                .cloned()
                .map(Into::into)
        }),
        Operation::UpdateMethodResponse => update(
            api_mut(core, r),
            r,
            &["methodResponses", r.s("statusCode")],
            METHOD_RESPONSE_FIELDS,
        ),
        Operation::DeleteMethodResponse => {
            delete(api_mut(core, r), r, &["methodResponses", r.s("statusCode")])
        }
        Operation::PutIntegrationResponse => put_integration_response(api_mut(core, r), r),
        Operation::GetIntegrationResponse => api(core, r).and_then(|d| {
            at(
                d,
                r,
                &[
                    "methodIntegration",
                    "integrationResponses",
                    r.s("statusCode"),
                ],
            )
            .cloned()
            .map(Into::into)
        }),
        Operation::UpdateIntegrationResponse => update(
            api_mut(core, r),
            r,
            &[
                "methodIntegration",
                "integrationResponses",
                r.s("statusCode"),
            ],
            INTEGRATION_RESPONSE_FIELDS,
        ),
        Operation::DeleteIntegrationResponse => delete(
            api_mut(core, r),
            r,
            &[
                "methodIntegration",
                "integrationResponses",
                r.s("statusCode"),
            ],
        ),
        _ => return None,
    })
}

const METHODS: &str = "GET,PUT,POST,DELETE,PATCH,OPTIONS,HEAD,ANY";

/// The HTTP method a request names (`PutIntegration` calls its label `requestHttpMethod` on the wire).
fn http_method(r: &Req) -> &str {
    r.opt("requestHttpMethod")
        .unwrap_or_else(|| r.s("httpMethod"))
}

/// The method's own path below the API document, then `below` inside it.
fn pointer(r: &Req, below: &[&str]) -> String {
    let mut p = format!(
        "/resources/{}/resourceMethods/{}",
        r.s("resourceId"),
        http_method(r)
    );
    for seg in below {
        p.push('/');
        p.push_str(&seg.replace('~', "~0").replace('/', "~1"));
    }
    p
}

/// Why nothing is at `below` of the method: the resource, the method, or the thing itself is missing (recorded words for the first two).
fn missing(doc: &Value, r: &Req, below: &[&str]) -> ApiError {
    if doc
        .pointer(&format!("/resources/{}", r.s("resourceId")))
        .is_none()
    {
        return invalid_id("Resource");
    }
    if doc.pointer(&pointer(r, &[])).is_none() {
        return invalid_id("Method");
    }
    match below {
        ["methodIntegration"] => invalid_id("Integration"),
        _ => invalid_id("Response status code"),
    }
}

fn at<'a>(doc: &'a Value, r: &Req, below: &[&str]) -> Result<&'a Value, ApiError> {
    doc.pointer(&pointer(r, below))
        .ok_or_else(|| missing(doc, r, below))
}

fn put_method(doc: Result<&mut Value, ApiError>, r: &Req) -> Res {
    let doc = doc?;
    let m = http_method(r);
    if !METHODS.split(',').any(|x| x == m) {
        return Err(bad(format!(
            "Invalid HttpMethod specified. Valid options are {METHODS}"
        )));
    }
    let res_path = format!("/resources/{}", r.s("resourceId"));
    if doc.pointer(&res_path).is_none() {
        return Err(invalid_id("Resource"));
    }
    if doc.pointer(&pointer(r, &[])).is_some() {
        return Err(conflict("Method already exists for this resource"));
    }
    check_method(doc, &r.body)?;
    let mut method = Map::new();
    method.insert("httpMethod".into(), json!(m));
    copy(
        &mut method,
        &r.body,
        &[
            "authorizationType",
            "authorizerId",
            "requestValidatorId",
            "operationName",
            "requestParameters",
            "requestModels",
            "authorizationScopes",
        ],
    );
    method.insert(
        "apiKeyRequired".into(),
        json!(r.flag("apiKeyRequired").unwrap_or(false)),
    );
    let method = Value::Object(method);
    if let Some(res) = doc.pointer_mut(&res_path)
        && let Some(ms) = child(res, "resourceMethods")
    {
        // WHY: the record and the answer are two values.
        ms.insert(m.to_owned(), method.clone());
    }
    Ok(method.into())
}

/// The authorization type, the authorizer and the validator a method names must make sense together `(memory)`.
fn check_method(doc: &Value, m: &Value) -> Result<(), ApiError> {
    let kind = text(m, "authorizationType").unwrap_or("NONE");
    if !matches!(kind, "NONE" | "AWS_IAM" | "CUSTOM" | "COGNITO_USER_POOLS") {
        return Err(bad(
            "Invalid authorization type specified. Valid options are NONE,AWS_IAM,CUSTOM,COGNITO_USER_POOLS",
        ));
    }
    if matches!(kind, "CUSTOM" | "COGNITO_USER_POOLS") {
        let id = text(m, "authorizerId").unwrap_or_default();
        if doc.pointer(&format!("/authorizers/{id}")).is_none() || id.is_empty() {
            return Err(bad(
                "Invalid authorizer ID specified. Setting the authorization type to CUSTOM or COGNITO_USER_POOLS requires a valid authorizer.",
            ));
        }
    }
    if let Some(v) = text(m, "requestValidatorId")
        && doc.pointer(&format!("/validators/{v}")).is_none()
    {
        return Err(bad("Invalid Request Validator identifier specified"));
    }
    Ok(())
}

fn put_integration(doc: Result<&mut Value, ApiError>, r: &Req) -> Res {
    let doc = doc?;
    at(doc, r, &[])?;
    let kind = r.s("type");
    let mut i = Map::new();
    i.insert("type".into(), json!(kind));
    copy(
        &mut i,
        &r.body,
        &[
            "httpMethod",
            "uri",
            "connectionType",
            "connectionId",
            "credentials",
            "requestParameters",
            "requestTemplates",
            "contentHandling",
            "tlsConfig",
            "integrationTarget",
        ],
    );
    if kind != "MOCK" {
        check_backend(kind, r)?;
        i.entry("connectionType")
            .or_insert_with(|| json!("INTERNET"));
    }
    let defaults = [
        ("passthroughBehavior", json!("WHEN_NO_MATCH")),
        (
            "timeoutInMillis",
            json!(r.int("timeoutInMillis").unwrap_or(29_000)),
        ),
        (
            "cacheNamespace",
            json!(r.opt("cacheNamespace").unwrap_or(r.s("resourceId"))),
        ),
        (
            "cacheKeyParameters",
            r.value("cacheKeyParameters")
                .cloned()
                .unwrap_or_else(|| json!([])),
        ),
        (
            "responseTransferMode",
            json!(r.opt("responseTransferMode").unwrap_or("BUFFERED")),
        ),
    ];
    for (k, v) in defaults {
        let v = if k == "passthroughBehavior" {
            r.value(k).cloned().unwrap_or(v)
        } else {
            v
        };
        i.insert(k.into(), v);
    }
    let integration = Value::Object(i);
    if let Some(method) = doc.pointer_mut(&pointer(r, &[])) {
        // WHY: the record and the answer are two values.
        put(method, "methodIntegration", integration.clone());
    }
    Ok(integration.into())
}

/// What a backend integration must name `(memory)` for the words.
fn check_backend(kind: &str, r: &Req) -> Result<(), ApiError> {
    if r.opt("httpMethod").is_none_or(str::is_empty) {
        return Err(bad("Enumeration value for HttpMethod must be non-empty"));
    }
    let uri = r.s("uri");
    if uri.is_empty() {
        return Err(bad("Invalid integration URI specified"));
    }
    if kind == "AWS_PROXY" && !(uri.contains(":lambda:path/") || uri.contains(":firehose:")) {
        return Err(bad(
            "Integrations of type 'AWS_PROXY' currently only supports Lambda function and Firehose stream invocations.",
        ));
    }
    if matches!(kind, "HTTP" | "HTTP_PROXY")
        && !(uri.starts_with("http://") || uri.starts_with("https://"))
    {
        return Err(bad("Invalid HTTP endpoint specified for URI"));
    }
    Ok(())
}

fn put_method_response(doc: Result<&mut Value, ApiError>, r: &Req) -> Res {
    let doc = doc?;
    at(doc, r, &[])?;
    let code = r.s("statusCode");
    let mut resp = Map::new();
    resp.insert("statusCode".into(), json!(code));
    copy(
        &mut resp,
        &r.body,
        &["responseParameters", "responseModels"],
    );
    let resp = Value::Object(resp);
    if let Some(method) = doc.pointer_mut(&pointer(r, &[]))
        && let Some(rs) = child(method, "methodResponses")
    {
        rs.insert(code.to_owned(), resp.clone());
    }
    Ok(resp.into())
}

fn put_integration_response(doc: Result<&mut Value, ApiError>, r: &Req) -> Res {
    let doc = doc?;
    at(doc, r, &["methodIntegration"])?;
    let code = r.s("statusCode");
    let has_response = at(doc, r, &[])
        .ok()
        .and_then(|m| obj(m, "methodResponses"))
        .is_some_and(|rs| rs.contains_key(code));
    if !has_response {
        return Err(invalid_id("Response status code"));
    }
    let mut resp = Map::new();
    copy(
        &mut resp,
        &r.body,
        &[
            "selectionPattern",
            "responseParameters",
            "responseTemplates",
            "contentHandling",
        ],
    );
    resp.insert("statusCode".into(), json!(code));
    let resp = Value::Object(resp);
    if let Some(i) = doc.pointer_mut(&pointer(r, &["methodIntegration"]))
        && let Some(rs) = child(i, "integrationResponses")
    {
        rs.insert(code.to_owned(), resp.clone());
    }
    Ok(resp.into())
}

const METHOD_FIELDS: &[Field] = &[
    f("/authorizationType", Kind::Str),
    f("/authorizerId", Kind::Str),
    f("/apiKeyRequired", Kind::Bool),
    f("/requestValidatorId", Kind::Str),
    f("/operationName", Kind::Str),
    f("/requestParameters", Kind::MapBool),
    f("/requestModels", Kind::Map),
    f("/authorizationScopes", Kind::List),
];

const INTEGRATION_FIELDS: &[Field] = &[
    f("/httpMethod", Kind::Str),
    f("/uri", Kind::Str),
    f("/connectionType", Kind::Str),
    f("/connectionId", Kind::Str),
    f("/credentials", Kind::Str),
    f("/requestParameters", Kind::Map),
    f("/requestTemplates", Kind::Map),
    f("/passthroughBehavior", Kind::Str),
    f("/contentHandling", Kind::Str),
    f("/timeoutInMillis", Kind::Int),
    f("/cacheNamespace", Kind::Str),
    f("/cacheKeyParameters", Kind::List),
    f("/tlsConfig/insecureSkipVerification", Kind::Bool),
    f("/responseTransferMode", Kind::Str),
    f("/integrationTarget", Kind::Str),
];

const METHOD_RESPONSE_FIELDS: &[Field] = &[
    f("/responseParameters", Kind::MapBool),
    f("/responseModels", Kind::Map),
];

const INTEGRATION_RESPONSE_FIELDS: &[Field] = &[
    f("/selectionPattern", Kind::Str),
    f("/responseParameters", Kind::Map),
    f("/responseTemplates", Kind::Map),
    f("/contentHandling", Kind::Str),
];

fn update(doc: Result<&mut Value, ApiError>, r: &Req, below: &[&str], fields: &[Field]) -> Res {
    let doc = doc?;
    let mut target = at(doc, r, below)?.clone();
    apply(&mut target, &r.patches(), fields)?;
    if below.is_empty() {
        check_method(doc, &target)?;
    }
    if let Some(slot) = doc.pointer_mut(&pointer(r, below)) {
        // WHY: the record and the answer are two values.
        *slot = target.clone();
    }
    Ok(target.into())
}

fn delete(doc: Result<&mut Value, ApiError>, r: &Req, below: &[&str]) -> Res {
    let doc = doc?;
    at(doc, r, below)?;
    let (parent, last) = match below.split_last() {
        Some((last, rest)) => (pointer(r, rest), (*last).to_owned()),
        None => (
            format!("/resources/{}/resourceMethods", r.s("resourceId")),
            http_method(r).to_owned(),
        ),
    };
    if let Some(p) = doc.pointer_mut(&parent).and_then(Value::as_object_mut) {
        p.remove(&last);
    }
    none()
}
