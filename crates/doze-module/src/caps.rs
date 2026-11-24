//! Optional capabilities. Registering one on the server (`serve(m).converge()`) requires its trait, and the
//! manifest's capability bits come from what was registered, so a module cannot advertise what it does not do.

use crate::module::{Cx, Module, Result};
use doze_module_proto::msg::blocks::{Children, ValueAnswer, check};
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

    /// Runs an action whose form has fields (protocol minor 2). The SDK has checked `values` against the action's
    /// declared fields (required, typed, in range) and filled in their defaults before this is called; refuse a value
    /// it cannot check with [`crate::forms::field_error`] so the form marks that field. By default the values are
    /// ignored and [`Admin::run_action`] runs, so a module without forms needs nothing here.
    fn run_form(
        &self,
        cx: &Cx<Self::Spec>,
        action: &str,
        resource: &str,
        input: Option<&str>,
        values: &BTreeMap<String, String>,
    ) -> impl Future<Output = Result<String>> + Send {
        let _ = values;
        self.run_action(cx, action, resource, input)
    }
}

/// A console value viewer (`serve(m).values()`, capability VALUE): one value in full, from the cells of the row
/// selected in `panel`, from byte `offset`, `max` bytes at most ([`ValueAnswer::slice`] does the arithmetic). The
/// SDK cuts a longer body and marks that more follows.
pub trait Values: Module {
    fn value(
        &self,
        cx: &Cx<Self::Spec>,
        panel: &str,
        row: &BTreeMap<String, String>,
        offset: u64,
        max: u32,
    ) -> impl Future<Output = Result<ValueAnswer>> + Send;
}

/// A console tree (`serve(m).tree()`, capability TREE): one page of the children of `parent` ("" for the top) in a
/// panel of kind "tree", filtered by what the person typed (search everything, not only what is open). Pages keep a
/// folder with a million children quick; [`Children::page`] pages a list at hand.
pub trait Tree: Module {
    fn children(
        &self,
        cx: &Cx<Self::Spec>,
        panel: &str,
        parent: &str,
        filter: &str,
        cursor: Option<&str>,
    ) -> impl Future<Output = Result<Children>> + Send;
}

/// A check for a "checks" panel (a resource of kind [`check::KIND`]): what was checked, "ok", "warn" or "bad", the
/// result in plain words, and the action that mends it, if one does. List checks with the other resources.
pub fn check(name: &str, status: &str, detail: &str, fix: Option<&str>) -> Resource {
    let mut info = BTreeMap::from([(check::DETAIL.to_string(), detail.to_string())]);
    if let Some(fix) = fix {
        info.insert(check::FIX.to_string(), fix.to_string());
    }
    Resource {
        kind: check::KIND.into(),
        name: name.into(),
        status: status.into(),
        info,
    }
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

/// Takes over client connections (the postgres filter: TLS, startup, cancel), boots the backend through the host,
/// and splices. The connection stays with the module until `accept` returns.
pub trait Wire: Module {
    fn accept(
        &self,
        conn: tokio::net::TcpStream,
        req: &doze_module_proto::msg::console::WireAccept,
        host: &WireHost,
    ) -> impl Future<Output = Result<()>> + Send;
}

/// The host's side of one hand-off: valid while that `wire.accept` call runs.
#[derive(Debug)]
pub struct WireHost {
    pub(crate) peer: doze_module_proto::peer::Peer,
    pub(crate) for_call: u32,
    pub(crate) instance: String,
}

impl WireHost {
    async fn use_(
        &self,
        m: doze_module_proto::method::Method,
    ) -> Result<doze_module_proto::msg::calls::Unit> {
        let a = doze_module_proto::msg::calls::WireUse {
            instance: self.instance.clone(),
        };
        let opts = doze_module_proto::peer::CallOpts {
            for_call: Some(self.for_call),
            ..Default::default()
        };
        self.peer.call(m, &a, opts).await
    }

    /// Boots the backend (lazily, only when this connection needs it) and says where it listens.
    pub async fn boot(&self) -> Result<doze_module_proto::msg::instance::Address> {
        let a = doze_module_proto::msg::calls::WireUse {
            instance: self.instance.clone(),
        };
        let opts = doze_module_proto::peer::CallOpts {
            for_call: Some(self.for_call),
            ..Default::default()
        };
        self.peer
            .call(doze_module_proto::method::Method::WireBoot, &a, opts)
            .await
    }

    /// A connection started splicing (the instance is in use).
    pub async fn acquire(&self) -> Result<()> {
        self.use_(doze_module_proto::method::Method::WireAcquire)
            .await
            .map(drop)
    }

    /// The spliced connection ended.
    pub async fn release(&self) -> Result<()> {
        self.use_(doze_module_proto::method::Method::WireRelease)
            .await
            .map(drop)
    }
}

/// Moves an instance's data to the version its block now declares (postgres's dump and restore between majors). The
/// host has stopped the instance; `cx.fetch()` gets the builds it needs, the old one included. What it returns is said
/// to the person who asked.
pub trait Migrate: Module {
    fn migrate(
        &self,
        cx: &Cx<Self::Spec>,
        pins: &[doze_module_proto::msg::run::Pin],
        target: &str,
    ) -> impl Future<Output = Result<String>> + Send;
}
