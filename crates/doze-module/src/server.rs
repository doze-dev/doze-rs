//! The module side of protocol 2: `serve(module)` builds the manifest (schema from the derive, capabilities from what is
//! registered), says hello, and dispatches calls to the module's traits until the host goes.

use crate::module::{Cx, Module, Result};
use crate::spec::{Decoding, Spec};
use doze_module_proto::method::Method;
use doze_module_proto::msg::calls::Unit;
use doze_module_proto::msg::error::{Error, ErrorKind};
use doze_module_proto::msg::hello::{Hello, Manifest, WireVersion};
use doze_module_proto::msg::instance::{Instance, Toolchain};
use doze_module_proto::msg::run::{ResolveArgs, WithToolchain};
use doze_module_proto::peer::{self, Handler, Incoming, Peer};
use doze_module_proto::version;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::watch;

pub(crate) type Route<M> =
    Arc<dyn Fn(Arc<M>, Incoming, Peer) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// A module ready to serve. Register capabilities, then `run`.
pub struct Server<M: Module> {
    pub(crate) module: Arc<M>,
    pub(crate) caps: u64,
    pub(crate) routes: HashMap<Method, Route<M>>,
    release: String,
}

impl<M: Module> std::fmt::Debug for Server<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("caps", &self.caps)
            .finish_non_exhaustive()
    }
}

/// Starts building the server for `module`.
pub fn serve<M: Module>(module: M) -> Server<M> {
    Server {
        module: Arc::new(module),
        caps: 0,
        routes: HashMap::new(),
        release: String::from("0.0.0"),
    }
}

/// Decodes the spec of `instance` and runs the module's own checks: a context, or the call back with the error.
pub(crate) fn make_cx<M: Module>(
    m: &M,
    instance: Instance,
    call: Incoming,
    peer: Peer,
) -> std::result::Result<Cx<M::Spec>, Box<(Incoming, Error)>> {
    let mut d = Decoding::new(&instance.version);
    let spec = M::Spec::decode(&instance.spec, &mut d);
    let mut diagnostics = d.diagnostics;
    if let Some(s) = &spec {
        diagnostics.extend(m.check(s, &instance));
    }
    match spec {
        Some(spec) if diagnostics.is_empty() => Ok(Cx {
            spec,
            instance,
            call,
            peer,
        }),
        _ => {
            let what = format!(
                "the {} block `{}` is not valid",
                instance.engine, instance.name
            );
            Err(Box::new((call, Error::invalid(what, diagnostics))))
        }
    }
}

/// Answers the call `cx` holds with `r`.
pub(crate) fn finish<S, T: minicbor::Encode<()>>(cx: Cx<S>, r: Result<T>) {
    let responder = cx.call.responder;
    let _ = match r {
        Ok(v) => responder.ok(&v),
        Err(e) => responder.err(&e),
    };
}

/// Runs `$body` with a context for `$instance`, answering with its result; a spec that does not decode is answered
/// with its diagnostics instead.
#[macro_export]
#[doc(hidden)]
macro_rules! with_cx {
    ($m:expr, $instance:expr, $call:expr, $peer:expr, |$cx:ident| $body:expr) => {
        match $crate::server::make_cx(&*$m, $instance, $call, $peer) {
            Err(b) => {
                let (call, e) = *b;
                drop(call.responder.err(&e))
            }
            Ok($cx) => {
                let r = $body;
                $crate::server::finish($cx, r);
            }
        }
    };
}

fn args<T: for<'b> minicbor::Decode<'b, ()>>(call: Incoming) -> Option<(T, Incoming)> {
    match call.args::<T>() {
        Ok(a) => Some((a, call)),
        Err(e) => {
            let _ = call.responder.err(&e);
            None
        }
    }
}

async fn required<M: Module>(m: Arc<M>, call: Incoming, peer: Peer) {
    match call.method {
        Method::Decode => {
            let Some((inst, call)) = args::<Instance>(call) else {
                return;
            };
            with_cx!(m, inst, call, peer, |cx| Ok::<_, Error>(Unit {}));
        }
        Method::Network => {
            let Some((inst, call)) = args::<Instance>(call) else {
                return;
            };
            with_cx!(m, inst, call, peer, |cx| {
                let listeners = m.network(&cx);
                let errs = doze_module_proto::network::validate(&listeners);
                if errs.is_empty() {
                    Ok(listeners)
                } else {
                    // A module bug, not the user's config: say so plainly.
                    Err(Error::new(
                        ErrorKind::Internal,
                        format!("the module declared invalid listeners: {}", errs.join("; ")),
                    ))
                }
            });
        }
        Method::Connection => {
            let Some((inst, call)) = args::<Instance>(call) else {
                return;
            };
            with_cx!(m, inst, call, peer, |cx| m.connection(&cx));
        }
        Method::Resolve => {
            let Some((a, call)) = args::<ResolveArgs>(call) else {
                return;
            };
            with_cx!(m, a.instance, call, peer, |cx| m
                .resolve(&cx, &a.pins, &a.target)
                .await);
        }
        Method::Provision | Method::Plan => toolchain_call(m, call, peer).await,
        Method::Shutdown => {
            let _ = call.responder.ok(&Unit {});
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            std::process::exit(0);
        }
        other => {
            let e = Error::new(
                ErrorKind::Unsupported,
                format!("this module does not offer {other:?}"),
            );
            let _ = call.responder.err(&e);
        }
    }
}

async fn toolchain_call<M: Module>(m: Arc<M>, call: Incoming, peer: Peer) {
    let method = call.method;
    let Some((a, call)) = args::<WithToolchain>(call) else {
        return;
    };
    let tc: Toolchain = a.toolchain;
    if method == Method::Provision {
        with_cx!(m, a.instance, call, peer, |cx| m.provision(&cx, &tc).await);
    } else {
        with_cx!(m, a.instance, call, peer, |cx| m.plan(&cx, &tc).await);
    }
}

impl<M: Module> Server<M> {
    /// The module's release, shown by the host ("0.3.0"); pass `env!("CARGO_PKG_VERSION")`.
    pub fn release(mut self, v: &str) -> Self {
        self.release = v.to_string();
        self
    }

    /// The manifest this server will send.
    pub fn manifest(&self) -> Manifest {
        let a = self.module.about();
        Manifest {
            engine: a.engine,
            category: a.category,
            title: a.title,
            tagline: a.tagline,
            versions: a.versions,
            exact_dots: a.exact_dots,
            default_port: a.default_port,
            boot_budget_ms: a
                .boot_budget
                .map(|d| u32::try_from(d.as_millis()).unwrap_or(u32::MAX)),
            supervised: a.supervised,
            backend_socket: a.backend_socket,
            caps: self.caps,
            schema: M::Spec::schema(),
            example: a.example,
            console: a.console,
            actions: a.actions,
            targets: Vec::new(),
        }
    }

    fn handler(&self, peer: watch::Receiver<Option<Peer>>) -> Handler {
        let (m, routes) = (self.module.clone(), Arc::new(self.routes.clone()));
        Arc::new(move |call: Incoming| {
            let (m, routes, mut peer) = (m.clone(), routes.clone(), peer.clone());
            Box::pin(async move {
                // Calls can arrive the moment the host has answered hello, before connect_host has returned.
                let Ok(p) = peer.wait_for(Option::is_some).await.map(|p| p.clone()) else {
                    return;
                };
                let Some(p) = p else { return };
                match routes.get(&call.method) {
                    Some(route) => route(m, call, p).await,
                    None => required(m, call, p).await,
                }
            })
        })
    }

    /// Serves on `ch` until the host goes. For tests and in-process modules; a module binary uses [`Server::run`].
    pub async fn serve_on(self, ch: doze_os::channel::Channel) -> Result<()> {
        let (tx, rx) = watch::channel(None);
        let hello = Hello {
            version: WireVersion {
                major: version::CURRENT.major,
                minor: version::CURRENT.minor,
            },
            module: self.module.about().engine,
            release: self.release.clone(),
            manifest: self.manifest(),
        };
        let (peer, _welcome) = peer::connect_host(ch, hello, self.handler(rx)).await?;
        let _ = tx.send(Some(peer.clone()));
        peer.closed().await;
        Ok(())
    }

    /// The module binary's main: takes the channel doze started it with, serves, and exits when the host goes.
    pub fn run(self) -> std::process::ExitCode {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => return crate::exit_with(&format!("cannot start: {e}")),
        };
        let r = rt.block_on(async {
            let ch = doze_os::channel::module_channel()
                .map_err(|e| Error::new(ErrorKind::Unavailable, e.to_string()))?;
            self.serve_on(ch).await
        });
        match r {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => crate::exit_with(&e.message),
        }
    }
}
