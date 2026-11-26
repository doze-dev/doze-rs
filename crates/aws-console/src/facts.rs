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
    /// The bus the services call each other on: the Resources space reads and changes them through it, as the caller
    /// `console`. `None` leaves the space without data (it says so).
    pub peers: Option<Arc<aws_peers::Peers>>,
    /// Every setting it was started with, in the order `doze-aws config print` lists them (Settings shows them).
    pub flags: Vec<Flag>,
    /// The data directory, as `doze-aws data` reads and resets it; `None` when the binary did not hand it over.
    pub data: Option<Arc<dyn LocalData>>,
    /// The Lab's fault rules: the table the gateway asks, or why there is none.
    pub lab: LabFacts,
}

/// What the binary says of the Lab (`--lab`).
#[derive(Debug, Clone, Default)]
pub struct LabFacts {
    /// The table the gateway asks about each request and the console makes rules in; `None` when the Lab is off.
    pub table: Option<Arc<crate::lab::LabTable>>,
    /// Why it is off, in a sentence (`--lab=false`, no console, a console open to other machines).
    pub off_why: Option<String>,
    /// The Lab is on and the console is served beyond this machine: anyone who can reach the console can inject faults.
    pub open: bool,
}

/// One setting of the run: its name, its value and where the value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flag {
    /// The flag's name without its dashes (`iam`).
    pub name: String,
    /// The value; a list's items joined with commas. A secret (`secret-key`) is never here: it is `None`.
    pub value: Option<String>,
    /// Where it came from, as a person looks for it (`--iam`, `DOZE_AWS_IAM`, `doze-aws.toml, line 4`, `the default`).
    pub source: String,
    /// Whether it is the default (nobody set it).
    pub default: bool,
}

/// The data directory as the binary's data engine sees it (`doze-aws data`): what each service keeps, and a reset.
pub trait LocalData: Send + Sync + std::fmt::Debug {
    /// `GET /_doze/data`'s answer: `{data_dir, bytes, services: [{name, title, bytes, ...}]}`.
    fn report(&self) -> aws_wire::service::BoxFuture<'_, Result<serde_json::Value, String>>;
    /// `POST /_doze/data/reset`'s answer for every service (`{services: [{service, items, bytes, freed}]}`); with `dry_run`,
    /// what it would remove, removing nothing.
    fn reset(
        &self,
        dry_run: bool,
    ) -> aws_wire::service::BoxFuture<'_, Result<serde_json::Value, String>>;
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

    /// The catalogue name of the service a bus caller named itself by: its catalogue name or a signing name of one of its APIs
    /// (`sns`, `states`, `events`); `None` for a name no service of the build has.
    #[must_use]
    pub fn catalogue_name(&self, said: &str) -> Option<&'static str> {
        self.services
            .iter()
            .find(|s| s.name == said || s.apis.iter().any(|a| a.meta().signing_name == said))
            .map(|s| s.name)
    }

    /// The service of the build whose API has SDK id `sdk_id`.
    #[must_use]
    pub fn api(&self, sdk_id: &str) -> Option<&Arc<dyn Service>> {
        self.services
            .iter()
            .flat_map(|s| s.apis.iter())
            .find(|a| a.meta().sdk_id == sdk_id)
    }
}
