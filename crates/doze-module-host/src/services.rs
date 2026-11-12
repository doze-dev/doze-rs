//! What the host offers a module while a call runs: downloads for `resolve` (the core's mirror, cache, checksums and
//! lock live behind [`Downloads`]) and the backend for `wire.accept` ([`Backends`]). Logs and events go to [`Notices`].

use doze_module_proto::frame::Kind;
use doze_module_proto::method::Method;
use doze_module_proto::msg::calls::{Unit, WireUse};
use doze_module_proto::msg::console::{Event, LogRecord};
use doze_module_proto::msg::error::{Error, ErrorKind};
use doze_module_proto::msg::instance::Address;
use doze_module_proto::msg::run::{Ensure, Ensured, ResolveMajor};
use doze_module_proto::peer::{self, Handler, Incoming};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A boxed future, for object-safe service traits.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The core's download service.
pub trait Downloads: Send + Sync + 'static {
    /// The full version the mirror maps `major` of `engine` to.
    fn resolve_major<'a>(
        &'a self,
        engine: &'a str,
        major: &'a str,
    ) -> BoxFuture<'a, Result<String, Error>>;
    /// Makes an exact build present and verified.
    fn ensure<'a>(&'a self, req: &'a Ensure) -> BoxFuture<'a, Result<Ensured, Error>>;
}

/// The core's side of a client hand-off: boot the backend lazily, count spliced connections for idle accounting.
pub trait Backends: Send + Sync + 'static {
    fn boot<'a>(&'a self, instance: &'a str) -> BoxFuture<'a, Result<Address, Error>>;
    fn acquire(&self, instance: &str);
    fn release(&self, instance: &str);
}

/// A log record or an event from a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    Log(LogRecord),
    Event(Event),
}

/// Where notices go: the module's engine name and the notice.
pub type Notices = Arc<dyn Fn(&str, Notice) + Send + Sync>;

/// What the host serves a module.
#[derive(Clone)]
pub struct Services {
    pub downloads: Arc<dyn Downloads>,
    pub backends: Option<Arc<dyn Backends>>,
    pub notices: Option<Notices>,
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Services")
            .field("backends", &self.backends.is_some())
            .finish_non_exhaustive()
    }
}

fn reply<T: minicbor::Encode<()>>(call: Incoming, r: Result<T, Error>) {
    let _ = match r {
        Ok(v) => call.responder.ok(&v),
        Err(e) => call.responder.err(&e),
    };
}

async fn serve(s: Services, call: Incoming) {
    match call.method {
        Method::FetchResolveMajor => match call.args::<ResolveMajor>() {
            Ok(a) => {
                let r = s.downloads.resolve_major(&a.engine, &a.major).await;
                reply(call, r);
            }
            Err(e) => reply::<Unit>(call, Err(e)),
        },
        Method::FetchEnsure => match call.args::<Ensure>() {
            Ok(a) => {
                let r = s.downloads.ensure(&a).await;
                reply(call, r);
            }
            Err(e) => reply::<Unit>(call, Err(e)),
        },
        Method::WireBoot | Method::WireAcquire | Method::WireRelease => wire(s, call).await,
        other => reply::<Unit>(
            call,
            Err(Error::new(
                ErrorKind::Unsupported,
                format!("the host does not serve {other:?}"),
            )),
        ),
    }
}

async fn wire(s: Services, call: Incoming) {
    let Some(b) = s.backends else {
        return reply::<Unit>(
            call,
            Err(Error::new(
                ErrorKind::Unsupported,
                "this host hands over no connections",
            )),
        );
    };
    let a = match call.args::<WireUse>() {
        Ok(a) => a,
        Err(e) => return reply::<Unit>(call, Err(e)),
    };
    match call.method {
        Method::WireBoot => {
            let r = b.boot(&a.instance).await;
            reply(call, r);
        }
        Method::WireAcquire => {
            b.acquire(&a.instance);
            reply(call, Ok(Unit {}));
        }
        _ => {
            b.release(&a.instance);
            reply(call, Ok(Unit {}));
        }
    }
}

/// The protocol handler for a module's calls to the host.
pub(crate) fn handler(s: Services) -> Handler {
    Arc::new(move |call| Box::pin(serve(s.clone(), call)))
}

/// The protocol's one-way frames, decoded and passed to `notices` with the engine's name.
pub(crate) fn notices(engine: String, sink: Option<Notices>) -> Option<peer::Notices> {
    let sink = sink?;
    Some(Arc::new(move |kind, body| {
        let notice = match kind {
            Kind::Log => doze_module_proto::msg::decode::<LogRecord>(&body)
                .ok()
                .map(Notice::Log),
            // An event this build does not know decodes as nothing and is dropped (protocol.md: events evolve).
            _ => doze_module_proto::msg::decode::<Event>(&body)
                .ok()
                .map(Notice::Event),
        };
        if let Some(n) = notice {
            sink(&engine, n);
        }
    }))
}
