//! Optional capabilities. Registering one on the server (`serve(m).converge()`) requires its trait, and the
//! manifest's capability bits come from what was registered, so a module cannot advertise what it does not do.

use crate::module::{Cx, Module, Result};
use doze_module_proto::msg::console::{Resource, Row};
use doze_module_proto::msg::instance::{Object, Toolchain};
use doze_module_proto::msg::run::Restart;
use doze_module_proto::msg::value::Value;
use doze_module_proto::peer::Responder;
use std::collections::BTreeMap;
use std::future::Future;

/// Makes a running instance match its declared structure (roles, databases, topics).
pub trait Converge: Module {
    fn converge(
        &self,
        cx: &Cx<Self::Spec>,
        tc: &Toolchain,
    ) -> impl Future<Output = Result<()>> + Send;
}

/// The structural objects an instance declares, for `doze plan` and `apply` (from the config, no live query).
pub trait Objects: Module {
    fn objects(&self, cx: &Cx<Self::Spec>) -> Result<Vec<Object>>;
}

/// Drops objects no longer declared.
pub trait Prune: Module {
    fn prune(
        &self,
        cx: &Cx<Self::Spec>,
        tc: &Toolchain,
        removed: &[Object],
    ) -> impl Future<Output = Result<()>> + Send;
}

/// Attributes beyond the baseline, under the instance's reference (`postgres.db.owner`).
pub trait Attributes: Module {
    fn attributes(&self, cx: &Cx<Self::Spec>) -> Result<BTreeMap<String, Value>>;
}

/// A URL a local process uses to reach the backend directly (a dependent's view).
pub trait BackendUrl: Module {
    fn backend_url(&self, cx: &Cx<Self::Spec>) -> Result<String>;
}

/// A shared, provisioned template that instances clone for instant cold boots.
pub trait Template: Module {
    fn ensure_template(
        &self,
        tc: &Toolchain,
        dir: &std::path::Path,
    ) -> impl Future<Output = Result<()>> + Send;
}

/// Liveness of a supervised instance.
pub trait Health: Module {
    fn health(&self, cx: &Cx<Self::Spec>) -> impl Future<Output = Result<()>> + Send;
}

/// What to do when a supervised instance exits unexpectedly.
pub trait Restarts: Module {
    fn restart(&self, cx: &Cx<Self::Spec>) -> Restart;
}

/// Sub-resources and data actions (purge a queue, cancel a query).
pub trait Admin: Module {
    fn resources(&self, cx: &Cx<Self::Spec>) -> impl Future<Output = Result<Vec<Resource>>> + Send;
    fn run_action(
        &self,
        cx: &Cx<Self::Spec>,
        action: &str,
        resource: &str,
        input: Option<&str>,
    ) -> impl Future<Output = Result<String>> + Send;
}

/// Rows for a console stream panel, sent through `out` (which waits for the host's credit).
pub trait Console: Module {
    fn tail(
        &self,
        cx: &Cx<Self::Spec>,
        panel: &str,
        selector: &str,
        out: &Rows<'_>,
    ) -> impl Future<Output = Result<()>> + Send;
}

/// Where `Console::tail` sends rows.
#[derive(Debug)]
pub struct Rows<'a>(pub(crate) &'a Responder);

impl Rows<'_> {
    /// Sends one row, waiting for credit; an error means the host stopped reading.
    pub async fn send(&self, row: &Row) -> Result<()> {
        self.0.item(row).await
    }
}
