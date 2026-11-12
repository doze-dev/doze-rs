//! Authorization on the plane: IAM (`AWS_IAM`, asked of the IAM authorizer as `execute-api:Invoke` on the method's ARN, with the REST
//! API's resource policy), Lambda authorizers (REST `TOKEN` and `REQUEST`, HTTP API `REQUEST` in payload 1.0 or 2.0, simple or IAM
//! policy answers) with AWS's cache, and JWT (see [`super::jwt`]). The answers and their words: 401 `Unauthorized` without an identity
//! or for a function that says `Unauthorized`, 403 for a denial, 500 with a null message for a function that fails.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use aws_peers::{Mode, Need};
use aws_wire::service::{Caller, WebReply};
use serde_json::{Map, Value, json};

use super::event::{self, Context};
use super::mapping::{plain, rest_source, v2_source};
use super::{Found, Incoming, Route, lambda, reply};
use crate::Inner;
use crate::doc::text;

/// Answers kept per authorizer and identity, for the authorizer's TTL. At most this many are kept.
const CACHE_MAX: usize = 1024;

/// The authorizer cache. A lock on a small leaf map: an actor would cost a task and a hop per request for a lookup.
#[derive(Debug, Default)]
pub(crate) struct Cache(Mutex<HashMap<String, (i64, Value)>>);

impl Cache {
    fn get(&self, key: &str, now: i64) -> Option<Value> {
        let map = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        map.get(key)
            .filter(|(until, _)| *until > now)
            .map(|(_, v)| v.clone())
    }

    fn put(&self, key: String, until: i64, v: Value, now: i64) {
        let mut map = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        map.retain(|_, (u, _)| *u > now);
        if map.len() >= CACHE_MAX
            && let Some(oldest) = map
                .iter()
                .min_by_key(|(_, (u, _))| *u)
                .map(|(k, _)| k.clone())
        {
            map.remove(&oldest);
        }
        map.insert(key, (until, v));
    }

    /// Drops every answer (an authorizer was changed, or a stage's authorizer cache flushed).
    pub(crate) fn clear(&self) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

fn unauthorized(found: &Found, req: &Incoming) -> WebReply {
    if found.kind == super::Kind::Rest {
        reply::gateway(found, "UNAUTHORIZED", 401, "Unauthorized", req, None)
    } else {
        reply::message(401, "Unauthorized")
    }
}

fn failed(found: &Found, req: &Incoming, why: &str) -> WebReply {
    tracing::warn!("execute-api {}: authorizer error: {why}", found.api_id());
    if found.kind == super::Kind::Rest {
        reply::gateway(found, "AUTHORIZER_FAILURE", 500, "null", req, None)
    } else {
        reply::message(500, "Internal Server Error")
    }
}

/// `AWS_IAM`: the request must be signed; the signer must be allowed `execute-api:Invoke` on the method (under `--iam`). The signature
/// itself is not checked on this plane.
pub(crate) async fn iam(
    inner: &Inner,
    found: &Found,
    req: &Incoming,
    arn: &str,
    extra: &mut Context,
) -> Result<(), WebReply> {
    let auth = req.header("authorization").unwrap_or_default();
    let Some(key) = auth
        .strip_prefix("AWS4-HMAC-SHA256 ")
        .and_then(|a| a.split("Credential=").nth(1))
        .and_then(|c| c.split('/').next())
    else {
        return Err(reply::gateway(
            found,
            "MISSING_AUTHENTICATION_TOKEN",
            403,
            "Missing Authentication Token",
            req,
            None,
        ));
    };
    extra.identity.insert("accessKey".into(), json!(key));
    let Some(peers) = inner.peers.as_ref().filter(|p| p.mode() != Mode::Off) else {
        return Ok(());
    };
    let caller = Caller {
        access_key_id: key.to_owned(),
        account_id: found.account.clone(),
        region: found.region.clone(),
        endpoint: String::new(),
        service: None,
        session_token: req.header("x-amz-security-token").map(str::to_owned),
    };
    let policy = text(&found.api, "policy").map(str::to_owned);
    let need = Need::new("execute-api:Invoke", arn).with_policy(policy);
    match peers.authorize(&caller, vec![need]).await {
        Ok(()) => Ok(()),
        Err(e) if peers.mode() == Mode::Soft => {
            tracing::warn!(
                "iam soft mode: this request would be refused: {}",
                e.message
            );
            Ok(())
        }
        Err(e) => Err(reply::gateway(
            found,
            "ACCESS_DENIED",
            403,
            &e.message,
            req,
            None,
        )),
    }
}

/// The values a REST `REQUEST` authorizer's identity sources name; `None` when one is missing.
fn rest_identity(
    auth: &Value,
    req: &Incoming,
    route: &Route,
    found: &Found,
) -> Option<Vec<String>> {
    let vars = found
        .variables()
        .map_or(Value::Null, |m| Value::Object(m.clone()));
    let ctx = json!({"identity": {"sourceIp": "127.0.0.1", "userAgent": req.header("user-agent")}});
    text(auth, "identitySource")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| rest_source(s, req, &route.params, &ctx, &vars).filter(|v| !v.is_empty()))
        .collect()
}

/// A REST API's Lambda authorizer (`TOKEN` or `REQUEST`).
pub(crate) async fn lambda_v1(
    inner: &Inner,
    found: &Found,
    route: &Route,
    req: &Incoming,
    arn: &str,
    extra: &mut Context,
) -> Result<(), WebReply> {
    let Some(auth) = route.authorizer.as_ref() else {
        return Err(failed(
            found,
            req,
            "the method names an authorizer that does not exist",
        ));
    };
    let token_kind = text(auth, "type") == Some("TOKEN");
    let identity =
        rest_identity(auth, req, route, found).ok_or_else(|| unauthorized(found, req))?;
    let event = if token_kind {
        let token = identity.first().cloned().unwrap_or_default();
        if let Some(expr) = text(auth, "identityValidationExpression").filter(|e| !e.is_empty())
            && !regex_lite::Regex::new(&format!("^(?:{expr})$")).is_ok_and(|r| r.is_match(&token))
        {
            return Err(unauthorized(found, req));
        }
        json!({"type": "TOKEN", "authorizationToken": token, "methodArn": arn})
    } else {
        let mut e = event::v1(found, route, req, &Context::default());
        crate::doc::put(&mut e, "type", json!("REQUEST"));
        crate::doc::put(&mut e, "methodArn", json!(arn));
        e
    };
    let answer = cached_call(inner, found, req, auth, &identity, &event).await?;
    if let Some(key) = text(&answer, "usageIdentifierKey") {
        extra.usage_key = Some(key.to_owned());
    }
    let principal = text(&answer, "principalId").map(str::to_owned);
    let (Some(principal), Some(policy)) = (principal, answer.get("policyDocument")) else {
        return Err(failed(
            found,
            req,
            "the authorizer's answer has no principalId or policyDocument",
        ));
    };
    allowed(found, req, policy, arn)?;
    let mut ctx = Map::new();
    for (k, v) in answer
        .get("context")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        ctx.insert(k.clone(), json!(plain(v)));
    }
    ctx.insert("principalId".into(), json!(principal));
    ctx.insert("integrationLatency".into(), json!(0));
    extra.authorizer = Some(Value::Object(ctx));
    Ok(())
}

/// Invokes an authorizer's function, or takes its cached answer. `Unauthorized` from the function is a 401.
async fn cached_call(
    inner: &Inner,
    found: &Found,
    req: &Incoming,
    auth: &Value,
    identity: &[String],
    event: &Value,
) -> Result<Value, WebReply> {
    let ttl = auth
        .get("authorizerResultTtlInSeconds")
        .and_then(Value::as_i64)
        .unwrap_or(300);
    let key = format!(
        "{}/{}/{}",
        found.api_id(),
        text(auth, "id")
            .or_else(|| text(auth, "authorizerId"))
            .unwrap_or_default(),
        identity.join("\u{0}")
    );
    if ttl > 0
        && !identity.is_empty()
        && let Some(hit) = inner.plane.cache.get(&key, req.now_ms)
    {
        return Ok(hit);
    }
    let uri = text(auth, "authorizerUri").unwrap_or_default();
    let function = lambda::function_of(uri, found.variables())
        .ok_or_else(|| failed(found, req, "the authorizer URI names no function"))?;
    let source = format!(
        "arn:aws:execute-api:{}:{}:{}/authorizers/{}",
        found.region,
        found.account,
        found.api_id(),
        text(auth, "id").unwrap_or_default()
    );
    let out = lambda::invoke(
        inner,
        &found.account,
        &function,
        &source,
        event.to_string().as_bytes(),
    )
    .await
    .map_err(|why| failed(found, req, &why))?;
    let answer: Value = serde_json::from_slice(&out.payload).unwrap_or(Value::Null);
    if out.error.is_some() {
        if text(&answer, "errorMessage") == Some("Unauthorized") {
            return Err(unauthorized(found, req));
        }
        return Err(failed(found, req, "the authorizer function failed"));
    }
    if ttl > 0 && !identity.is_empty() {
        inner
            .plane
            .cache
            .put(key, req.now_ms + ttl * 1000, answer.clone(), req.now_ms);
    }
    Ok(answer)
}

/// Whether an authorizer's policy allows `execute-api:Invoke` on the method: an explicit deny wins, an allow must match.
fn allowed(found: &Found, req: &Incoming, policy: &Value, arn: &str) -> Result<(), WebReply> {
    let text = if policy.is_string() {
        plain(policy)
    } else {
        policy.to_string()
    };
    let doc = match aws_iam_policy::parse(&text) {
        Some(Ok(d)) => d,
        _ => {
            return Err(failed(
                found,
                req,
                "the authorizer's policy document cannot be read",
            ));
        }
    };
    let subject = aws_iam_policy::Subject {
        arns: Vec::new(),
        account: found.account.clone(),
        service: None,
    };
    let context = aws_iam_policy::Context::new();
    let docs = [&doc];
    let policies = aws_iam_policy::Policies {
        identity: &docs,
        ..aws_iam_policy::Policies::default()
    };
    let request = aws_iam_policy::Request {
        action: "execute-api:Invoke",
        resource: arn,
        context: &context,
        subject: &subject,
    };
    let decision = aws_iam_policy::evaluate(&policies, &request);
    if decision.allowed() {
        return Ok(());
    }
    let explicit = decision.verdict == aws_iam_policy::Verdict::ExplicitDeny;
    let msg = if explicit {
        "User is not authorized to access this resource with an explicit deny"
    } else {
        "User is not authorized to access this resource"
    };
    if found.kind == super::Kind::Rest {
        Err(reply::gateway(found, "ACCESS_DENIED", 403, msg, req, None))
    } else {
        Err(reply::message(403, "Forbidden"))
    }
}

/// An HTTP API's Lambda authorizer.
pub(crate) async fn lambda_v2(
    inner: &Inner,
    found: &Found,
    route: &Route,
    req: &Incoming,
    extra: &mut Context,
) -> Result<(), WebReply> {
    let Some(auth) = route.authorizer.as_ref() else {
        return Err(failed(
            found,
            req,
            "the route names an authorizer that does not exist",
        ));
    };
    let vars = found
        .variables()
        .map_or(Value::Null, |m| Value::Object(m.clone()));
    let raw = event::raw_path(found);
    let ctx = json!({"identity": {"sourceIp": "127.0.0.1", "userAgent": req.header("user-agent")}});
    let mut identity = Vec::new();
    for src in auth
        .get("identitySource")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let v = v2_source(src, req, &route.params, &raw, &ctx, &vars);
        if v.is_empty() {
            return Err(unauthorized(found, req));
        }
        identity.push(v);
    }
    let route_arn = format!(
        "arn:aws:execute-api:{}:{}:{}/{}/{}{}",
        found.region,
        found.account,
        found.api_id(),
        found.stage_name,
        req.method,
        found.path
    );
    let v2 = text(auth, "authorizerPayloadFormatVersion") == Some("2.0");
    let event = if v2 {
        let mut e = event::v2(found, route, req, &Context::default());
        crate::doc::put(&mut e, "type", json!("REQUEST"));
        crate::doc::put(&mut e, "routeArn", json!(route_arn));
        crate::doc::put(&mut e, "identitySource", json!(identity));
        e
    } else {
        let mut e = event::v1(found, route, req, &Context::default());
        crate::doc::put(&mut e, "type", json!("REQUEST"));
        crate::doc::put(&mut e, "methodArn", json!(route_arn));
        e
    };
    let answer = cached_call(inner, found, req, auth, &identity, &event).await?;
    let simple = auth.get("enableSimpleResponses").and_then(Value::as_bool) == Some(true);
    let context = answer.get("context").cloned().unwrap_or_else(|| json!({}));
    if simple {
        if answer.get("isAuthorized").and_then(Value::as_bool) != Some(true) {
            return Err(reply::message(403, "Forbidden"));
        }
    } else {
        let policy = answer
            .get("policyDocument")
            .ok_or_else(|| failed(found, req, "the authorizer's answer has no policyDocument"))?;
        allowed(found, req, policy, &route_arn)?;
    }
    extra.authorizer = Some(if v2 {
        json!({"lambda": context})
    } else {
        let mut c = context.as_object().cloned().unwrap_or_default();
        if let Some(p) = answer.get("principalId") {
            c.insert("principalId".into(), p.clone());
        }
        c.insert("integrationLatency".into(), json!(0));
        Value::Object(c)
    });
    Ok(())
}
