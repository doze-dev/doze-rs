//! The `Module` trait every module implements, the static facts it states (`About`), and the context its handlers
//! receive (`Cx`).

use crate::fetch::Fetch;
use crate::spec::Spec;
use doze_module_proto::frame::Kind;
use doze_module_proto::msg::console::{Action, ConsoleSpec, Event, LogRecord};
use doze_module_proto::msg::error::{Diagnostic, Error};
use doze_module_proto::msg::instance::{Connection, Instance, Listener, Toolchain};
use doze_module_proto::msg::run::{Plan, Provisioned, Resolved};
use doze_module_proto::peer::{Cancel, Peer};
use std::future::Future;
use std::time::Duration;

/// A call's result.
pub type Result<T> = std::result::Result<T, Error>;

/// Static facts about a module (the manifest, minus what the SDK derives: schema and capabilities).
#[derive(Debug, Clone, Default)]
pub struct About {
    pub engine: String,
    pub category: String,
    pub title: String,
    pub tagline: String,
    /// Selectable majors, oldest first; empty for a versionless engine.
    pub versions: Vec<String>,
    /// Dots that make a version exact ("8.1.8": 2).
    pub exact_dots: u8,
    pub default_port: u16,
    pub boot_budget: Option<Duration>,
    pub supervised: bool,
    /// The backend socket's file name in the instance's socket directory (`{port}` is filled in).
    pub backend_socket: Option<String>,
    /// A complete example block, checked by the module's acceptance tests.
    pub example: String,
    pub console: Option<ConsoleSpec>,
    pub actions: Vec<Action>,
    /// The targets the module supports (PLAN.md D12); releases publish only these, and only where acceptance passed.
    pub targets: Vec<String>,
    /// For the registry page: a paragraph, links, and the label the example uses.
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub source: Option<String>,
    pub example_label: Option<String>,
    /// What protocol 1's core hard-coded per engine: the version `doze add` writes, the shell client, and the signs
    /// `doze init` looks for (see the manifest fields of the same names).
    pub default_version: Option<String>,
    pub client: Option<doze_module_proto::msg::hello::ClientCommand>,
    pub detect: Option<doze_module_proto::msg::hello::Detect>,
}

/// What a handler is given: the decoded spec, the instance, and the call it serves.
#[derive(Debug)]
pub struct Cx<S> {
    pub spec: S,
    pub instance: Instance,
    pub(crate) call: doze_module_proto::peer::Incoming,
    pub(crate) peer: Peer,
}

impl<S> Cx<S> {
    /// Fires when the host cancels the call or its deadline passes.
    pub fn cancel(&self) -> &Cancel {
        &self.call.cancel
    }

    /// Sends a log record about this instance to the host.
    pub fn log(&self, level: u8, message: impl Into<String>) {
        let r = LogRecord {
            level,
            message: message.into(),
            instance: Some(self.instance.name.clone()),
            fields: Default::default(),
        };
        let _ = self.peer.notify(Kind::Log, &r);
    }

    /// Reports progress of this call (a download) for the CLI and the app.
    pub fn progress(&self, done: u64, total: u64, what: impl Into<String>) {
        let e = Event::Progress {
            for_call: self.call.id(),
            done,
            total,
            what: what.into(),
        };
        let _ = self.peer.notify(Kind::Event, &e);
    }

    /// The host's download service; valid while this `resolve` call runs.
    pub fn fetch(&self) -> Fetch {
        Fetch {
            peer: self.peer.clone(),
            for_call: self.call.id(),
        }
    }
}

/// A doze module. The required calls are here; the optional ones are capability traits (`caps`).
pub trait Module: Send + Sync + 'static {
    /// The configuration block, decoded from the core's evaluated values.
    type Spec: Spec + Send + Sync + 'static;

    fn about(&self) -> About;

    /// Rules the schema cannot state (one of two arguments, a value range), as diagnostics.
    fn check(&self, _spec: &Self::Spec, _instance: &Instance) -> Vec<Diagnostic> {
        Vec::new()
    }

    /// The listeners the instance wants; by default one TCP listener, "main".
    fn network(&self, _cx: &Cx<Self::Spec>) -> Vec<Listener> {
        vec![Listener {
            name: "main".into(),
            port: 0,
            http: false,
            routes: Vec::new(),
            tcp: false,
        }]
    }

    /// The connection string doze shows for the instance.
    fn connection(&self, cx: &Cx<Self::Spec>) -> Result<Connection>;

    /// Finds the toolchain, asking the host (`cx.fetch()`) to download and verify it.
    fn resolve(
        &self,
        cx: &Cx<Self::Spec>,
        pins: &[doze_module_proto::msg::run::Pin],
        target: &str,
    ) -> impl Future<Output = Result<Resolved>> + Send;

    /// Makes the data directory ready to boot; idempotent.
    fn provision(
        &self,
        cx: &Cx<Self::Spec>,
        tc: &Toolchain,
    ) -> impl Future<Output = Result<Provisioned>> + Send;

    /// How to run the instance.
    fn plan(
        &self,
        cx: &Cx<Self::Spec>,
        tc: &Toolchain,
    ) -> impl Future<Output = Result<Plan>> + Send;
}
