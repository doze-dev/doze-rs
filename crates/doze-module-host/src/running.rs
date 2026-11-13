//! One running module process: launched through doze-os, introduced by its hello, called through typed methods.

use crate::services::{self, Services};
use doze_module_proto::method::Method;
use doze_module_proto::msg::calls::{
    PruneArgs, RunAction, Shutdown, TemplateClone, TemplateEnsure, Unit,
};
use doze_module_proto::msg::console::{Resource, TailArgs, WireAccept};
use doze_module_proto::msg::error::{Error, ErrorKind};
use doze_module_proto::msg::hello::{Hello, Manifest};
use doze_module_proto::msg::instance::{Connection, Instance, Listener, Object, Toolchain};
use doze_module_proto::msg::run::{
    Plan, Provisioned, ResolveArgs, Resolved, Restart, WithToolchain,
};
use doze_module_proto::msg::value::Value;
use doze_module_proto::peer::{self, CallOpts, Peer, Stream};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Deadlines the host gives each kind of call.
const QUICK: Duration = Duration::from_secs(10);
const SLOW: Duration = Duration::from_secs(30 * 60);

/// A module process the host talks to.
#[derive(Debug)]
pub struct Running {
    pub engine: String,
    pub release: String,
    pub manifest: Manifest,
    peer: Peer,
    child: Mutex<Option<tokio::process::Child>>,
    last_used: Mutex<Instant>,
}

/// Starts `program` as the module for `engine` and waits for its hello.
pub async fn launch(engine: &str, program: &Path, services: Services) -> Result<Running, Error> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.kill_on_drop(true);
    let (child, ch) = doze_os::channel::spawn_module(cmd).await.map_err(|e| {
        Error::new(
            ErrorKind::Unavailable,
            format!("starting the {engine} module ({}): {e}", program.display()),
        )
    })?;
    let notices = services::notices(engine.to_string(), services.notices.clone());
    let (peer, hello): (Peer, Hello) =
        peer::accept_module(ch, services::handler(services), notices).await?;
    if hello.manifest.engine != engine {
        return Err(Error::new(
            ErrorKind::Invalid,
            format!(
                "{} is the {} module, not {engine}",
                program.display(),
                hello.manifest.engine
            ),
        ));
    }
    Ok(Running {
        engine: engine.to_string(),
        release: hello.release,
        manifest: hello.manifest,
        peer,
        child: Mutex::new(Some(child)),
        last_used: Mutex::new(Instant::now()),
    })
}

impl Running {
    async fn call<A, R>(&self, m: Method, args: &A, deadline: Duration) -> Result<R, Error>
    where
        A: minicbor::Encode<()>,
        R: for<'b> minicbor::Decode<'b, ()>,
    {
        if let Ok(mut t) = self.last_used.lock() {
            *t = Instant::now();
        }
        self.peer
            .call(
                m,
                args,
                CallOpts {
                    deadline: Some(deadline),
                    ..CallOpts::default()
                },
            )
            .await
    }

    /// Whether the module's channel is still open.
    pub fn is_alive(&self) -> bool {
        !self.peer.is_closed()
    }

    /// How long since the host last called it.
    pub fn idle_for(&self) -> Duration {
        self.last_used
            .lock()
            .map(|t| t.elapsed())
            .unwrap_or_default()
    }

    pub async fn decode(&self, i: &Instance) -> Result<(), Error> {
        self.call::<_, Unit>(Method::Decode, i, QUICK)
            .await
            .map(drop)
    }
    pub async fn network(&self, i: &Instance) -> Result<Vec<Listener>, Error> {
        self.call(Method::Network, i, QUICK).await
    }
    pub async fn connection(&self, i: &Instance) -> Result<Connection, Error> {
        self.call(Method::Connection, i, QUICK).await
    }
    pub async fn attributes(&self, i: &Instance) -> Result<BTreeMap<String, Value>, Error> {
        self.call(Method::Attributes, i, QUICK).await
    }
    pub async fn backend_url(&self, i: &Instance) -> Result<String, Error> {
        self.call(Method::BackendUrl, i, QUICK).await
    }
    pub async fn resolve(&self, a: &ResolveArgs) -> Result<Resolved, Error> {
        self.call(Method::Resolve, a, SLOW).await
    }
    pub async fn provision(&self, a: &WithToolchain) -> Result<Provisioned, Error> {
        self.call(Method::Provision, a, SLOW).await
    }
    pub async fn plan(&self, a: &WithToolchain) -> Result<Plan, Error> {
        self.call(Method::Plan, a, QUICK).await
    }
    pub async fn converge(&self, a: &WithToolchain) -> Result<(), Error> {
        self.call::<_, Unit>(Method::Converge, a, SLOW)
            .await
            .map(drop)
    }
    pub async fn objects(&self, i: &Instance) -> Result<Vec<Object>, Error> {
        self.call(Method::Objects, i, QUICK).await
    }
    pub async fn prune(&self, a: &PruneArgs) -> Result<(), Error> {
        self.call::<_, Unit>(Method::Prune, a, SLOW).await.map(drop)
    }
    pub async fn template_ensure(&self, tc: &Toolchain, dir: &str) -> Result<(), Error> {
        let a = TemplateEnsure {
            toolchain: tc.clone(),
            template_dir: dir.to_string(),
        };
        self.call::<_, Unit>(Method::TemplateEnsure, &a, SLOW)
            .await
            .map(drop)
    }
    pub async fn template_clone(&self, from: &str, to: &str) -> Result<(), Error> {
        let a = TemplateClone {
            template_dir: from.to_string(),
            dest_dir: to.to_string(),
        };
        self.call::<_, Unit>(Method::TemplateClone, &a, SLOW)
            .await
            .map(drop)
    }
    pub async fn health(&self, i: &Instance) -> Result<(), Error> {
        self.call::<_, Unit>(Method::Health, i, QUICK)
            .await
            .map(drop)
    }
    pub async fn restart_policy(&self, i: &Instance) -> Result<Restart, Error> {
        self.call(Method::Restart, i, QUICK).await
    }
    pub async fn resources(&self, i: &Instance) -> Result<Vec<Resource>, Error> {
        self.call(Method::Resources, i, QUICK).await
    }
    pub async fn run_action(&self, a: &RunAction) -> Result<String, Error> {
        self.call(Method::RunAction, a, SLOW).await
    }

    /// A console stream; drop it to stop.
    pub fn tail(&self, a: &TailArgs) -> Result<Stream, Error> {
        self.peer.stream(Method::Tail, a, CallOpts::default())
    }

    /// Hands a client connection to the module for its whole life; returns when the module is done with it.
    pub async fn wire_accept(
        &self,
        conn: std::net::TcpStream,
        a: &WireAccept,
    ) -> Result<(), Error> {
        let opts = CallOpts {
            handle: Some(doze_os::channel::Passable::from(conn)),
            ..CallOpts::default()
        };
        self.peer
            .call::<_, Unit>(Method::WireAccept, a, opts)
            .await
            .map(drop)
    }

    /// Asks the module to exit within `grace`, then makes sure it has.
    pub async fn shutdown(&self, grace: Duration) {
        let ms = u32::try_from(grace.as_millis()).unwrap_or(u32::MAX);
        let _ = self
            .call::<_, Unit>(Method::Shutdown, &Shutdown { grace_ms: ms }, grace)
            .await;
        let child = self.child.lock().ok().and_then(|mut c| c.take());
        if let Some(mut c) = child
            && tokio::time::timeout(grace, c.wait()).await.is_err()
        {
            let _ = c.start_kill();
        }
    }
}
