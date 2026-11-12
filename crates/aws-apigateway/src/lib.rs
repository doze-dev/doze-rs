//! API Gateway: REST APIs (v1, the `apigateway` model) and HTTP and WebSocket APIs (v2, `apigatewayv2`), and the execute-api plane
//! that serves what was deployed. Both APIs sign as `apigateway` and are two [`Service`]s over one actor per region, because a deployed
//! API is found by its id whichever API made it, and custom domain names are shared by both, as on AWS.
//!
//! **Control plane** ([`v1`], [`v2`]): each API is one JSON document in redb (`apigateway.redb`, one per region). A REST deployment is
//! a snapshot of the API, served by the stages that point at it; an HTTP API's auto-deploying stage serves the live API.
//!
//! **Execute-api plane** ([`plane`]): `/_aws/execute-api/<id>/<stage>/<path>` and `<id>.execute-api.<region>.<host>/<stage>/<path>`
//! (an HTTP API's `$default` stage at the root), unsigned, through [`aws_wire::service::Web`]: routing by AWS's precedence, Lambda
//! proxy (payload 1.0 and 2.0), Lambda and AWS service integrations with mapping templates (a VTL subset; what it cannot run is
//! refused by name), HTTP and HTTP proxy, MOCK, Lambda authorizers with their policy evaluated, JWT authorizers, IAM authorization,
//! API keys, request validation, CORS, stage variables, gateway responses, binary media types, access logs, and WebSocket APIs with
//! the `@connections` management API.
//!
//! **Stored, not active locally:** custom domain names and their mappings, usage plan throttles and quotas, stage caches, canary
//! settings, method throttling. **Refused by name** (501): client certificates, VPC links, SDK generation, documentation parts and
//! versions, usage reports, portals and routing rules, `TestInvoke*`.

mod authz;
mod core;
mod doc;
mod domains;
mod errors;
mod gwresp;
mod openapi;
mod patch;
mod plane;
mod req;
mod tags;
mod v1;
mod v2;

use std::path::PathBuf;
use std::sync::Arc;

use aws_peers::Peers;
use aws_schema::json::Violation;
use aws_schema::meta::ServiceMeta;
use aws_schema::patterns::Patterns;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::handler::reply_json;
use aws_wire::rest::{RestReply, RestService, Upload};
use aws_wire::service::{BoxFuture, Call, Reply, Service, Web, WebReply, WebRequest};
use doze_runtime::{Clock, Ctx, Keyed};

use crate::core::{Answer, Api, Ask, Core};

/// The region whose files are kept in the service's directory itself.
const HOME_REGION: &str = "us-east-1";

/// What both APIs share: the actors, and what the execute-api plane needs beside them.
#[derive(Debug)]
pub(crate) struct Inner {
    actors: Keyed<Core>,
    pub(crate) dir: PathBuf,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) plane: plane::State,
    patterns_v1: Patterns,
    patterns_v2: Patterns,
}

/// The REST API service (v1). It also owns the execute-api addresses, for APIs of both kinds.
#[derive(Debug, Clone)]
pub struct ApiGateway {
    inner: Arc<Inner>,
}

/// The HTTP and WebSocket API service (v2).
#[derive(Debug, Clone)]
pub struct ApiGatewayV2 {
    inner: Arc<Inner>,
}

/// Registers both APIs over one set of actors. `ctx` says where the files live and how the time is read.
#[must_use]
pub fn services(ctx: Arc<Ctx>) -> (ApiGateway, ApiGatewayV2) {
    let peers = ctx
        .ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<Peers>().ok());
    let inner = Arc::new(Inner {
        actors: Keyed::new("apigateway", Arc::clone(&ctx), HOME_REGION),
        dir: ctx.dir.clone(),
        clock: Arc::clone(&ctx.clock),
        peers,
        plane: plane::State::default(),
        patterns_v1: Patterns::new(&aws_model_api_gateway::tables::TABLES),
        patterns_v2: Patterns::new(&aws_model_apigatewayv2::tables::TABLES),
    });
    (
        ApiGateway {
            inner: Arc::clone(&inner),
        },
        ApiGatewayV2 { inner },
    )
}

impl Inner {
    /// Asks the actor of `region`.
    pub(crate) async fn ask(&self, region: &str, ask: Ask) -> Result<Answer, ApiError> {
        let actor = self
            .actors
            .actor(region)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        actor
            .call(ask)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))
    }

    /// The regions that have files: the home region and every `regions/<name>` directory.
    pub(crate) fn regions(&self) -> Vec<String> {
        let mut out = vec![HOME_REGION.to_owned()];
        if let Ok(dirs) = std::fs::read_dir(self.dir.join("regions")) {
            out.extend(
                dirs.filter_map(Result::ok)
                    .filter_map(|e| e.file_name().into_string().ok()),
            );
        }
        out
    }

    async fn run(&self, api: Api, call: Call) -> Result<RestReply, ApiError> {
        let region = call.caller.region.clone();
        let op = call.op;
        let answer = self.ask(&region, Ask::Call(api, Box::new(call))).await?;
        let Answer::Reply { reply, durable } = answer else {
            return Err(ApiError::internal(
                "API Gateway answered a call with something else",
            ));
        };
        if let Some(saved) = durable {
            saved
                .await
                .map_err(|e| ApiError::internal(format!("could not save: {e}")))?;
        }
        let out = reply?;
        self.plane.after_change(api, op);
        Ok(RestReply {
            json: out.json,
            status: out.status,
            body: None,
        })
    }
}

/// Words a request that breaks the model's rules as API Gateway does: `BadRequestException` (v2's protocol enum as recorded on
/// 2026-11-09; the rest `(memory)`).
fn invalid(tables: &Tables, violation: &Violation) -> ApiError {
    let mut e = aws_wire::service::default_invalid(tables, violation);
    e.code = "BadRequestException".into();
    if let Violation::Enum { member, .. } = violation
        && tables
            .member(*member)
            .is_some_and(|m| m.name == "ProtocolType")
    {
        e.message = "Invalid protocol specified. Must be one of [HTTP, WEBSOCKET]".into();
    }
    e
}

macro_rules! service {
    ($ty:ident, $api:expr, $model:ident, $patterns:ident, $built:path) => {
        impl Service for $ty {
            fn meta(&self) -> &'static ServiceMeta {
                &$model::meta::META
            }

            fn tables(&self) -> &'static Tables {
                &$model::tables::TABLES
            }

            fn serves(&self, op: u32) -> bool {
                $model::Operation::from_index(op).is_some_and($built)
            }

            /// A call from a peer (CloudFormation): the same operations, answered as the model's output.
            fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
                Box::pin(async move {
                    let op = call.op;
                    let reply = self.inner.run($api, call).await?;
                    reply_json(
                        &$model::tables::TABLES,
                        &self.inner.$patterns,
                        op,
                        &reply.json,
                    )
                })
            }

            fn rest(&self) -> Option<&dyn RestService> {
                Some(self)
            }

            fn web(&self) -> Option<&dyn Web> {
                ($api == Api::V1).then_some(self as &dyn Web)
            }

            fn invalid(&self, violation: &Violation) -> ApiError {
                invalid(&$model::tables::TABLES, violation)
            }
        }

        impl RestService for $ty {
            fn call_rest(
                &self,
                call: Call,
                _upload: Option<Box<dyn Upload>>,
            ) -> BoxFuture<'_, Result<RestReply, ApiError>> {
                Box::pin(async move { self.inner.run($api, call).await })
            }
        }

        impl Web for $ty {
            fn owns(&self, host: &str, path: &str) -> bool {
                plane::owns(host, path)
            }

            fn serve(&self, request: WebRequest) -> BoxFuture<'_, WebReply> {
                Box::pin(plane::serve(Arc::clone(&self.inner), request))
            }

            fn upgrade(&self, request: WebRequest) -> BoxFuture<'_, aws_wire::service::Upgrade> {
                Box::pin(plane::ws::upgrade(Arc::clone(&self.inner), request))
            }
        }
    };
}

service!(
    ApiGateway,
    Api::V1,
    aws_model_api_gateway,
    patterns_v1,
    v1::built
);
service!(
    ApiGatewayV2,
    Api::V2,
    aws_model_apigatewayv2,
    patterns_v2,
    v2::built
);

/// The parsers, for the fuzz targets (`fuzz/`): each takes any input and must not panic.
#[doc(hidden)]
pub mod fuzz {
    /// A mapping template, read and run against an empty request.
    pub fn vtl(text: &str) {
        let mut env = crate::plane::vtl::Env::default();
        drop(crate::plane::vtl::render(text, &mut env));
    }

    /// An HTTP integration's answer.
    pub fn http_answer(raw: &[u8]) {
        crate::plane::client_parse(raw);
    }

    /// A JSON Web Token.
    pub fn jwt(text: &str) {
        crate::plane::jwt_decode(text);
    }

    /// WebSocket frames from a client.
    pub fn ws_frames(raw: &[u8]) {
        crate::plane::ws::read_all(raw);
    }

    /// An OpenAPI document (JSON or the YAML subset).
    pub fn openapi(text: &str) {
        drop(crate::openapi::read(text));
    }
}

#[cfg(test)]
mod tests;
