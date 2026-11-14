//! The execute-api plane: a deployed API answering plain HTTP. Requests arrive unsigned through [`aws_wire::service::Web`] at
//!
//! ```text
//! /_aws/execute-api/<api id>/<stage>/<path>              (an HTTP API's $default stage: /_aws/execute-api/<api id>/<path>)
//! <api id>.execute-api.<region>.<any host>/<stage>/<path>
//! ```
//!
//! The actor finds the method or route ([`find`], [`route`]); everything after (authorizers, keys, validation, the integration, the
//! answer) runs here, outside it, because an integration can take as long as a Lambda function runs.

mod access;
mod auth;
mod aws;
mod client;
mod cors;
mod custom;
mod event;
mod find;
mod httpapi;
mod jwt;
mod lambda;
mod mapping;
mod reply;
mod rest;
mod route;
mod validate;
pub(crate) mod vtl;
pub(crate) mod ws;

use std::sync::Arc;

use aws_wire::service::{WebReply, WebRequest};
use bytes::Bytes;
use serde_json::{Map, Value};

use crate::Inner;
use crate::core::{Answer, Api, Ask};

/// An HTTP answer read from bytes (for the fuzz target).
pub(crate) fn client_parse(raw: &[u8]) -> bool {
    client::parse(raw).is_ok()
}

/// A JSON Web Token read (for the fuzz target).
pub(crate) fn jwt_decode(text: &str) -> bool {
    jwt::decode(text).is_some()
}

/// What the plane keeps beside the actors: the authorizer cache and the open WebSocket connections.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) cache: auth::Cache,
    pub(crate) ws: ws::Connections,
}

impl State {
    /// After a control-plane change: an authorizer's cached answers go when it, or a stage's authorizer cache, is changed.
    pub(crate) fn after_change(&self, api: Api, op: u32) {
        let name = match api {
            Api::V1 => aws_model_api_gateway::Operation::from_index(op).map(|o| format!("{o:?}")),
            Api::V2 => aws_model_apigatewayv2::Operation::from_index(op).map(|o| format!("{o:?}")),
        };
        if name.is_some_and(|n| n.contains("Authorizer")) {
            self.cache.clear();
        }
    }
}

/// Which kind of API a request reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Rest,
    Http,
    WebSocket,
}

/// What the actor is asked to find.
#[derive(Debug)]
pub(crate) struct Find {
    pub(crate) api: String,
    /// The path after the API id, as sent.
    pub(crate) rest: String,
    pub(crate) method: String,
    /// Whether the stage is the path's first segment (always, except the host form of an HTTP API's `$default` stage).
    pub(crate) stage_in_path: bool,
    /// A WebSocket message's selected route key.
    pub(crate) route_key: Option<String>,
}

/// The method (REST) or route (HTTP, WebSocket) a request reached, copied out of the actor.
#[derive(Debug, Clone)]
pub(crate) struct Route {
    pub(crate) resource_id: String,
    /// The resource's path template (`/orders/{id}`) or the route key (`GET /orders/{id}`).
    pub(crate) resource_path: String,
    pub(crate) params: Vec<(String, String)>,
    /// The REST method (with `methodIntegration`) or the v2 route.
    pub(crate) method: Value,
    pub(crate) integration: Value,
    pub(crate) authorizer: Option<Value>,
    pub(crate) validator: Option<Value>,
    /// A WebSocket route's route responses, and its integration's responses.
    pub(crate) responses: Option<Value>,
    pub(crate) integration_responses: Option<Value>,
}

/// An API and stage a request reached, and the route if one matched.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub(crate) kind: Kind,
    pub(crate) account: String,
    pub(crate) region: String,
    pub(crate) api: Value,
    pub(crate) stage: Value,
    pub(crate) stage_name: String,
    /// The path below the stage, decoded, from `/`.
    pub(crate) path: String,
    pub(crate) route: Option<Route>,
    /// A REST API's own gateway responses, and its models.
    pub(crate) responses: Map<String, Value>,
    pub(crate) models: Map<String, Value>,
}

impl Found {
    pub(crate) fn api_id(&self) -> &str {
        crate::doc::text(&self.api, "id")
            .or_else(|| crate::doc::text(&self.api, "apiId"))
            .unwrap_or_default()
    }

    /// A stage variable map (`variables` on a REST stage, `stageVariables` on a v2 one).
    pub(crate) fn variables(&self) -> Option<&Map<String, Value>> {
        crate::doc::obj(&self.stage, "variables")
            .or_else(|| crate::doc::obj(&self.stage, "stageVariables"))
            .filter(|m| !m.is_empty())
    }
}

/// Why nothing was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Miss {
    NoApi,
    NoStage(Kind),
}

/// A request as the plane reads it.
#[derive(Debug)]
pub(crate) struct Incoming {
    pub(crate) method: String,
    pub(crate) query: Vec<(String, String)>,
    pub(crate) raw_query: String,
    /// Names lower-cased, in the order sent.
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Bytes,
    pub(crate) host: String,
    pub(crate) request_id: String,
    pub(crate) now_ms: i64,
}

impl Incoming {
    /// The first value of header `name` (lower case).
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub(crate) fn query_value(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Where a request is addressed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Addr {
    pub(crate) api: String,
    pub(crate) region: Option<String>,
    pub(crate) rest: String,
    via_host: bool,
}

const PREFIX: &str = "/_aws/execute-api/";

pub(crate) fn addr(host: &str, path: &str) -> Option<Addr> {
    if let Some(after) = path.strip_prefix(PREFIX) {
        let (api, rest) = after.split_once('/').unwrap_or((after, ""));
        return (!api.is_empty()).then(|| Addr {
            api: api.to_owned(),
            region: None,
            rest: rest.to_owned(),
            via_host: false,
        });
    }
    let name = host.split(':').next().unwrap_or(host);
    let mut labels = name.split('.');
    let api = labels.next()?;
    if labels.next()? != "execute-api" {
        return None;
    }
    let region = labels.next().filter(|r| r.contains('-')).map(str::to_owned);
    Some(Addr {
        api: api.to_owned(),
        region,
        rest: path.trim_start_matches('/').to_owned(),
        via_host: true,
    })
}

/// Whether a request is the plane's: an execute-api host or the `/_aws/execute-api/` path. The `@connections` management API at
/// those addresses is a signed REST service of its own (`management.rs`), routed by the gateway.
pub(crate) fn owns(host: &str, path: &str) -> bool {
    addr(host, path).is_some() && !path.contains("/@connections/")
}

/// Answers an execute-api request.
pub(crate) async fn serve(inner: Arc<Inner>, request: WebRequest) -> WebReply {
    let Some(addr) = addr(&request.host, &request.path) else {
        return reply::message(404, "Not Found");
    };
    let incoming = Incoming {
        method: request.method,
        query: crate::plane::mapping::query_pairs(&request.query),
        raw_query: request.query,
        headers: request.headers,
        body: request.body,
        host: request.host,
        request_id: aws_wire::RequestId::new().as_str().to_owned(),
        now_ms: inner.clock.now_ms(),
    };
    let find = Find {
        api: addr.api.clone(),
        rest: addr.rest.clone(),
        method: incoming.method.clone(),
        stage_in_path: true,
        route_key: None,
    };
    match locate(&inner, addr.region.as_deref(), find).await {
        Ok(found) => match found.kind {
            Kind::Rest => rest::run(&inner, found, incoming).await,
            Kind::Http => httpapi::run(&inner, found, incoming).await,
            Kind::WebSocket => reply::message(426, "Upgrade Required"),
        },
        Err(Miss::NoStage(Kind::Rest)) => reply::message(403, "Forbidden"),
        Err(_) => reply::message(404, "Not Found"),
    }
}

/// Finds the API in the region named, or in each region that has files.
pub(crate) async fn locate(inner: &Inner, region: Option<&str>, find: Find) -> Result<Found, Miss> {
    let regions = region.map_or_else(|| inner.regions(), |r| vec![r.to_owned()]);
    let find = Box::new(find);
    let mut miss = Miss::NoApi;
    for region in regions {
        let ask = Ask::Find(Box::new(Find {
            api: find.api.clone(),
            rest: find.rest.clone(),
            method: find.method.clone(),
            stage_in_path: find.stage_in_path,
            route_key: find.route_key.clone(),
        }));
        match inner.ask(&region, ask).await {
            Ok(Answer::Found(Ok(found))) => return Ok(*found),
            Ok(Answer::Found(Err(Miss::NoApi))) => {}
            Ok(Answer::Found(Err(m))) => miss = m,
            _ => {}
        }
        if miss != Miss::NoApi {
            break;
        }
    }
    Err(miss)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_address_forms_are_read() {
        assert_eq!(
            addr("localhost:4566", "/_aws/execute-api/abc/dev/orders/1"),
            Some(Addr {
                api: "abc".into(),
                region: None,
                rest: "dev/orders/1".into(),
                via_host: false
            })
        );
        let a = addr("abc.execute-api.eu-west-1.localhost:4566", "/dev/x").unwrap();
        assert_eq!(
            (a.api.as_str(), a.region.as_deref(), a.rest.as_str()),
            ("abc", Some("eu-west-1"), "dev/x")
        );
        assert!(addr("localhost", "/restapis").is_none());
    }
}
