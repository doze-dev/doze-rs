//! CloudFormation: templates in JSON or YAML (with the short-form tags), every intrinsic function, conditions, mappings, rules,
//! parameters (SSM-typed ones read from the local SSM), dynamic references, the SAM, language-extensions and include transforms,
//! stacks with their events, change sets, rollback, outputs and exports, nested stacks, and custom resources, as a state machine
//! behind a lazy actor per region.
//!
//! **Resources are made through the other services' own operations**, called over the peer bus as `cloudformation` acting for the
//! caller, so each keeps its own validation and wording. Each resource type is in one class (decision D5): **mapped** types are made;
//! **skipped** types (opaque infrastructure such as VPCs and certificates) are accepted with an identity of their own and their
//! status reason says they were not made; **refused** types (databases, compute, ...) are refused before anything is made. A
//! `Config` switch changes the default for unknown and refused types.
//!
//! **Operations run beside the actor.** A call that provisions answers at once, as AWS does, with the stack in progress; the work
//! runs as a task that reports each step back, so `DescribeStackEvents` shows it as it happens and independent resources are made
//! at the same time. An operation cut off by a stop is marked failed when the region next opens.

mod authz;
mod changes;
mod changesets;
mod core;
mod custom;
mod describe;
mod dynref;
mod engine;
mod errors;
mod j;
mod model;
mod nested;
mod notify;
mod prepare;
mod progress;
mod req;
mod res;
mod rollback;
mod stackpolicy;
mod stacks;
mod tmpl;
mod transform;
mod web;
mod yaml;
mod zip;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use aws_model_cloudformation::Operation;
use aws_model_cloudformation::meta::META;
use aws_model_cloudformation::tables::TABLES;
use aws_peers::Peers;
use aws_schema::json::Violation;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Reply, Service, Web, WebReply, WebRequest};
use doze_runtime::{Ctx, Keyed};

use crate::core::{Answer, Core, Request};
use crate::engine::Runner;
use crate::web::Responses;

/// The region whose files are kept in the service's directory itself.
const HOME_REGION: &str = "us-east-1";

/// What happens to a resource type that is not mapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Unsupported {
    /// Each type's own class: opaque infrastructure is skipped with a warning, behaviour-bearing types are refused (decision D5).
    #[default]
    Default,
    /// Every unmapped type is skipped with a warning on the resource.
    Skip,
    /// Every unmapped type is refused.
    Fail,
}

/// How CloudFormation behaves where a choice is the developer's.
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// The default for unmapped types.
    pub unsupported: Unsupported,
    /// Per-type overrides (`AWS::RDS::DBInstance` = skip), taking precedence over `unsupported`.
    pub overrides: Vec<(String, Unsupported)>,
    /// Name resources that have no name in the template after their logical id (as the Go edition did) instead of AWS's
    /// `<stack>-<logical>-<random>`.
    pub logical_names: bool,
}

/// What the host's context carries for this service.
pub(crate) struct Ext {
    pub(crate) peers: Arc<Peers>,
    pub(crate) config: Arc<Config>,
}

/// The CloudFormation service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Cloudformation {
    actors: Keyed<Core>,
    ctx: Arc<Ctx>,
    peers: Option<Arc<Peers>>,
    config: Arc<Config>,
    responses: Arc<Responses>,
}

impl std::fmt::Debug for Ext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ext")
    }
}

impl Cloudformation {
    /// Registers CloudFormation with the default configuration.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self::with_config(ctx, Config::default())
    }

    /// Registers CloudFormation. `ctx` says where its files live and carries the peer bus.
    #[must_use]
    pub fn with_config(ctx: Arc<Ctx>, config: Config) -> Self {
        let peers = ctx
            .ext
            .as_ref()
            .and_then(|e| Arc::clone(e).downcast::<Peers>().ok());
        let config = Arc::new(config);
        let ext: Option<Arc<dyn std::any::Any + Send + Sync>> = peers.as_ref().map(|p| {
            Arc::new(Ext {
                peers: Arc::clone(p),
                config: Arc::clone(&config),
            }) as Arc<dyn std::any::Any + Send + Sync>
        });
        let inner = Arc::new(Ctx {
            dir: ctx.dir.clone(),
            idle: ctx.idle,
            tasks: ctx.tasks.clone(),
            clock: Arc::clone(&ctx.clock),
            ext,
        });
        Self {
            actors: Keyed::new("cloudformation", Arc::clone(&inner), HOME_REGION),
            ctx: inner,
            peers,
            config,
            responses: Arc::new(Responses::default()),
        }
    }

    fn runner(&self, region: &str) -> Result<Arc<Runner>, String> {
        let actor = self.actors.actor(region).map_err(|e| e.to_string())?;
        Ok(Arc::new(Runner {
            actor,
            peers: self.peers.clone(),
            tasks: self.ctx.tasks.clone(),
            responses: Arc::clone(&self.responses),
            config: Arc::clone(&self.config),
            clock: Arc::clone(&self.ctx.clock),
        }))
    }

    async fn ask(&self, call: Call) -> Reply {
        let runner = match self.runner(&call.caller.region) {
            Ok(r) => r,
            Err(e) => return Err(ApiError::internal(e)),
        };
        match runner.ask(Request::Call(call)).await {
            Ok(Answer { reply, .. }) => reply,
            Err(e) => Err(ApiError::internal(e)),
        }
    }
}

impl Service for Cloudformation {
    fn meta(&self) -> &'static aws_schema::meta::ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static aws_schema::tables::Tables {
        &TABLES
    }

    fn serves(&self, op: u32) -> bool {
        Operation::from_index(op).is_some_and(core::built)
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(self.ask(call))
    }

    fn web(&self) -> Option<&dyn Web> {
        Some(self)
    }

    /// CloudFormation's Query front end words a failed model check as `ValidationError`.
    fn invalid(&self, violation: &Violation) -> ApiError {
        let mut e = aws_wire::service::default_invalid(&TABLES, violation);
        e.code = "ValidationError".into();
        e.shape = None;
        e
    }
}

/// Whether `text` is YAML the template reader accepts. Exists so the reader can be fuzzed.
#[doc(hidden)]
#[must_use]
pub fn check_yaml(text: &str) -> bool {
    yaml::parse(text).is_ok()
}

impl Web for Cloudformation {
    fn owns(&self, _host: &str, path: &str) -> bool {
        web::owns(path)
    }

    fn serve(&self, request: WebRequest) -> BoxFuture<'_, WebReply> {
        Box::pin(async move { self.responses.serve(&request) })
    }
}
