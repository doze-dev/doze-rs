//! What the binary tells the console about itself at start: the facts `GET /api/v1/about` answers with, and the services
//! themselves, which `GET /api/v1/services` counts operations on when it is asked (never before: nothing is tallied at start).

use std::sync::Arc;

use aws_wire::service::Service;

/// One service of the catalogue, as `--services` names it.
#[derive(Clone)]
pub struct ServiceFacts {
    /// The name in `--services` (`stepfunctions`).
    pub name: &'static str,
    /// The name a person reads (`Step Functions`).
    pub title: &'static str,
    /// Whether this run serves it (`--services`).
    pub served: bool,
    /// The APIs it answers (DynamoDB and DynamoDB Streams; API Gateway's three), each the service itself and not the stand-in a
    /// service left out of `--services` is replaced by, so the counts are of what the build can do.
    pub apis: Vec<Arc<dyn Service>>,
}

impl std::fmt::Debug for ServiceFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceFacts")
            .field("name", &self.name)
            .field("served", &self.served)
            .field("apis", &self.apis.len())
            .finish_non_exhaustive()
    }
}

/// This doze-aws, as it describes itself.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// The build's version.
    pub version: String,
    /// The account every request is for.
    pub account: String,
    /// The default region (`--region`).
    pub region: String,
    /// The URL AWS SDKs reach the gateway at.
    pub endpoint: String,
    /// `trust` or `verify`.
    pub auth: String,
    /// `off`, `soft` or `enforce`.
    pub iam: String,
    /// The data directory.
    pub data_dir: String,
    /// The process.
    pub pid: u32,
    /// When it started, in seconds since 1970.
    pub started: u64,
    /// Every service of the build, in catalogue order.
    pub services: Vec<ServiceFacts>,
}

impl Facts {
    /// The catalogue name of the service that signs as `signing_name` (`states` is `stepfunctions`); the signing name itself for
    /// one the catalogue does not hold.
    #[must_use]
    pub fn service_named(&self, signing_name: &'static str) -> &'static str {
        self.services
            .iter()
            .find(|s| s.apis.iter().any(|a| a.meta().signing_name == signing_name))
            .map_or(signing_name, |s| s.name)
    }
}
