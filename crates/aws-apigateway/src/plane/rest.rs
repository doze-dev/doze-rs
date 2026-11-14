//! A REST API serving one request, in the order AWS runs it: the method (403 `Missing Authentication Token` when there is none), the
//! authorizer, the API key, request validation, the integration, then the access log.

use aws_wire::service::WebReply;
use serde_json::Value;

use super::custom::{self, Backend};
use super::event::{self, Context};
use super::{Found, Incoming, Route, auth, lambda, reply, validate};
use crate::Inner;
use crate::doc::text;

/// `arn:aws:execute-api:<region>:<account>:<api>/<stage>/<METHOD>/<path>`: what an authorizer is told and IAM is asked about.
pub(crate) fn method_arn(found: &Found, method: &str) -> String {
    format!(
        "arn:aws:execute-api:{}:{}:{}/{}/{method}{}",
        found.region,
        found.account,
        found.api_id(),
        found.stage_name,
        found.path
    )
}

pub(crate) async fn run(inner: &Inner, found: Found, req: Incoming) -> WebReply {
    let started = std::time::Instant::now();
    let answer = serve(inner, &found, &req).await;
    super::access::log(inner, &found, &req, &answer, started).await;
    answer
}

async fn serve(inner: &Inner, found: &Found, req: &Incoming) -> WebReply {
    let Some(route) = found.route.as_ref() else {
        return reply::gateway(
            found,
            "MISSING_AUTHENTICATION_TOKEN",
            403,
            "Missing Authentication Token",
            req,
            None,
        );
    };
    let mut extra = Context::default();
    if let Err(denied) = authorize(inner, found, route, req, &mut extra).await {
        return denied;
    }
    if let Err(refused) = api_key(inner, found, route, req, &mut extra).await {
        return refused;
    }
    let context = event::rest_context(found, route, req, &extra);
    if let Err(invalid) = validate::rest(found, route, req, &context) {
        return invalid;
    }
    integrate(inner, found, route, req, &extra, &context).await
}

async fn authorize(
    inner: &Inner,
    found: &Found,
    route: &Route,
    req: &Incoming,
    extra: &mut Context,
) -> Result<(), WebReply> {
    let arn = method_arn(found, &req.method);
    match text(&route.method, "authorizationType").unwrap_or("NONE") {
        "AWS_IAM" => auth::iam(inner, found, req, &arn, extra).await,
        "CUSTOM" => auth::lambda_v1(inner, found, route, req, &arn, extra).await,
        "COGNITO_USER_POOLS" => super::jwt::cognito(found, route, req, extra),
        _ => Ok(()),
    }
}

/// A method that requires a key is served only for a key that exists, is enabled and is in a usage plan for this stage (`x-api-key`,
/// or the authorizer's `usageIdentifierKey` when the API's key source is `AUTHORIZER`); otherwise 403 `Forbidden`.
async fn api_key(
    inner: &Inner,
    found: &Found,
    route: &Route,
    req: &Incoming,
    extra: &mut Context,
) -> Result<(), WebReply> {
    if route.method.get("apiKeyRequired").and_then(Value::as_bool) != Some(true) {
        return Ok(());
    }
    let from_authorizer = text(&found.api, "apiKeySource") == Some("AUTHORIZER");
    let value = if from_authorizer {
        extra.usage_key.clone()
    } else {
        req.header("x-api-key").map(str::to_owned)
    };
    let forbidden = || reply::gateway(found, "INVALID_API_KEY", 403, "Forbidden", req, None);
    let value = value.ok_or_else(forbidden)?;
    let ask = crate::core::Ask::Key {
        value: value.clone(),
        api: found.api_id().to_owned(),
        stage: found.stage_name.clone(),
    };
    match inner.ask(&found.region, ask).await {
        Ok(crate::core::Answer::Key(Some(id))) => {
            extra.identity.insert("apiKey".into(), Value::String(value));
            extra.identity.insert("apiKeyId".into(), Value::String(id));
            Ok(())
        }
        _ => Err(forbidden()),
    }
}

async fn integrate(
    inner: &Inner,
    found: &Found,
    route: &Route,
    req: &Incoming,
    extra: &Context,
    context: &Value,
) -> WebReply {
    let integration = &route.integration;
    let uri = text(integration, "uri").unwrap_or_default();
    match text(integration, "type").unwrap_or("MOCK") {
        "MOCK" => custom::mock(found, route, req, context),
        "AWS_PROXY" if uri.contains(":lambda:path/") => {
            proxy(inner, found, route, req, extra).await
        }
        "AWS_PROXY" => custom::config_error(
            found,
            req,
            context,
            "an AWS_PROXY integration to Firehose is not built in doze-aws",
        ),
        "HTTP_PROXY" => super::client::proxy(found, route, req).await,
        "HTTP" => non_proxy(inner, found, route, req, context, Target::Http).await,
        _ if uri.contains(":lambda:path/") => {
            non_proxy(inner, found, route, req, context, Target::Lambda).await
        }
        _ => non_proxy(inner, found, route, req, context, Target::Service).await,
    }
}

/// A Lambda proxy integration: the payload-1.0 event, and the function's answer read by AWS's rules (502 when malformed).
async fn proxy(
    inner: &Inner,
    found: &Found,
    route: &Route,
    req: &Incoming,
    extra: &Context,
) -> WebReply {
    let uri = text(&route.integration, "uri").unwrap_or_default();
    let Some(function) = lambda::function_of(uri, found.variables()) else {
        return reply::gateway(
            found,
            "API_CONFIGURATION_ERROR",
            500,
            "Internal server error",
            req,
            None,
        );
    };
    let ev = event::v1(found, route, req, extra);
    let source = method_arn(found, &req.method);
    match lambda::invoke(
        inner,
        &found.account,
        &function,
        &source,
        ev.to_string().as_bytes(),
    )
    .await
    {
        Ok(out) if out.error.is_none() => match lambda::proxy_v1(&out.payload) {
            Ok(a) => reply::of(a.status, a.headers, a.body),
            Err(why) => {
                tracing::warn!(
                    "execute-api {}: Malformed Lambda proxy response: {why}",
                    found.api_id()
                );
                reply::gateway(
                    found,
                    "DEFAULT_5XX",
                    502,
                    "Internal server error",
                    req,
                    None,
                )
            }
        },
        Ok(_) => reply::gateway(
            found,
            "DEFAULT_5XX",
            502,
            "Internal server error",
            req,
            None,
        ),
        Err(why) => {
            tracing::warn!(
                "execute-api {}: the function could not be invoked: {why}",
                found.api_id()
            );
            reply::gateway(
                found,
                "DEFAULT_5XX",
                500,
                "Internal server error",
                req,
                None,
            )
        }
    }
}

#[derive(Clone, Copy)]
enum Target {
    Lambda,
    Http,
    Service,
}

async fn non_proxy(
    inner: &Inner,
    found: &Found,
    route: &Route,
    req: &Incoming,
    context: &Value,
    target: Target,
) -> WebReply {
    let mapped = match custom::request(found, route, req, context) {
        Ok(m) => m,
        Err(reply) => return reply,
    };
    let backend = match target {
        Target::Lambda => lambda_backend(inner, found, route, req, &mapped.body).await,
        Target::Http => super::client::send(found, route, req, &mapped).await,
        Target::Service => super::aws::rest(inner, found, route, req, &mapped).await,
    };
    match backend {
        Ok(b) => custom::respond(found, route, req, &mapped.context, b),
        Err(why) => custom::config_error(found, req, &mapped.context, &why),
    }
}

/// A Lambda custom integration: the mapped body is the event; a failure's `errorMessage` is what selection patterns match.
async fn lambda_backend(
    inner: &Inner,
    found: &Found,
    route: &Route,
    req: &Incoming,
    body: &str,
) -> Result<Backend, String> {
    let uri = text(&route.integration, "uri").unwrap_or_default();
    let function = lambda::function_of(uri, found.variables())
        .ok_or("the integration URI names no function")?;
    let payload = if body.trim().is_empty() { "{}" } else { body };
    let out = lambda::invoke(
        inner,
        &found.account,
        &function,
        &method_arn(found, &req.method),
        payload.as_bytes(),
    )
    .await?;
    let text = String::from_utf8_lossy(&out.payload).into_owned();
    let selector = if out.error.is_some() {
        serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("errorMessage")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    Ok(Backend {
        selector,
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: text,
    })
}
