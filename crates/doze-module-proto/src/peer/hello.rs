//! The first exchange. The module calls `hello` with its version and manifest; the host negotiates the version, keeps
//! the module's capabilities for the guards, and answers `welcome`. Nothing else is served by the host before it.

use super::{CallOpts, Handler, Notices, Peer, Side};
use crate::method::Method;
use crate::msg::error::{Error, ErrorKind};
use crate::msg::hello::{Hello, Welcome, WireVersion};
use crate::version::{self, Version};
use doze_os::channel::Channel;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

/// How long either side waits for the other's first word.
const HELLO_WAIT: Duration = Duration::from_secs(10);

/// Calls in flight the host allows a module, and log records per second it keeps.
const MAX_CALLS: u32 = 64;
const LOG_RATE: u32 = 200;

/// Module side: start the peer on the host's channel and introduce the module.
pub async fn connect_host(
    ch: Channel,
    hello: Hello,
    handler: Handler,
) -> Result<(Peer, Welcome), Error> {
    let peer = super::io::start(ch, Side::Module, handler, None);
    peer.inner.caps.store(hello.manifest.caps, Ordering::SeqCst);
    let opts = CallOpts {
        deadline: Some(HELLO_WAIT),
        ..CallOpts::default()
    };
    let welcome: Welcome = peer.call(Method::Hello, &hello, opts).await?;
    peer.inner
        .minor
        .store(welcome.version.minor, Ordering::SeqCst);
    Ok((peer, welcome))
}

/// Host side: start the peer on a module's channel and wait for its `hello`. Calls before it are refused.
pub async fn accept_module(
    ch: Channel,
    handler: Handler,
    notices: Option<Notices>,
) -> Result<(Peer, Hello), Error> {
    let (tx, rx) = oneshot::channel();
    let first = Arc::new(Mutex::new(Some(tx)));
    let route: Handler = Arc::new(move |call| {
        if call.method == Method::Hello
            && let Some(tx) = first.lock().ok().and_then(|mut f| f.take())
        {
            return Box::pin(async move { drop(tx.send(call)) });
        }
        handler(call)
    });
    let peer = super::io::start(ch, Side::Host, route, notices);
    let call = tokio::time::timeout(HELLO_WAIT, rx)
        .await
        .map_err(|_| Error::new(ErrorKind::TimedOut, "the module did not say hello"))?
        .map_err(|_| {
            Error::new(
                ErrorKind::Unavailable,
                "the module went away before saying hello",
            )
        })?;
    let hello: Hello = call.args()?;
    let ours = version::CURRENT;
    let theirs = Version {
        major: hello.version.major,
        minor: hello.version.minor,
    };
    let agreed = match version::negotiate(ours, theirs, true) {
        Ok(v) => v,
        Err(m) => {
            let e = Error::new(ErrorKind::Unsupported, m.to_string());
            let _ = call.responder.err(&e);
            return Err(e);
        }
    };
    peer.inner.caps.store(hello.manifest.caps, Ordering::SeqCst);
    peer.inner.minor.store(agreed.minor, Ordering::SeqCst);
    let welcome = Welcome {
        version: WireVersion {
            major: agreed.major,
            minor: agreed.minor,
        },
        max_calls: MAX_CALLS,
        log_rate: LOG_RATE,
    };
    call.responder.ok(&welcome)?;
    Ok((peer, hello))
}
