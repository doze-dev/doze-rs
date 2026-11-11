//! One connection: read a request, answer it, repeat.

use std::io;
use std::sync::Arc;

use bytes::BytesMut;
use kafka_auth::Authenticator;
use kafka_broker::{Broker, Listener};
use kafka_storage::fs::Fs;
use kafka_wire::WireError;
use kafka_wire::frame::{FrameDecoder, Trust};
use kafka_wire::messages::ApiKey;
use kafka_wire::profile::{Features, Profile};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::session::Session;
use crate::{Timeouts, frames};

mod exchange;

/// A read buffer starts this size and is given back to this size once a large request has been handled, so a
/// connection that sent one big Produce does not hold its memory for ever.
const BUFFER: usize = 8 << 10;
const KEEP: usize = 64 << 10;

/// What every connection of a server shares.
#[derive(Debug)]
pub(crate) struct Context<F: Fs> {
    pub(crate) broker: Arc<Broker<F>>,
    pub(crate) listener: Listener,
    pub(crate) profile: Profile,
    pub(crate) features: Features,
    pub(crate) timeouts: Timeouts,
    /// Who may log in. `None`: nobody is asked to, and a connection is served from its first request.
    pub(crate) auth: Option<Arc<Authenticator>>,
}

/// Why a connection ended. None of these texts holds anything a client sent.
#[derive(Debug, thiserror::Error)]
pub(crate) enum End {
    #[error("the client closed the connection")]
    PeerClosed,
    #[error("the server is stopping")]
    Stopped,
    #[error("idle for too long")]
    Idle,
    #[error("a request or a response took too long")]
    Slow,
    #[error("{0}")]
    Io(io::Error),
    #[error("{0}")]
    Wire(WireError),
    #[error("a {0:?} request cannot be answered")]
    Refused(ApiKey),
    #[error("a kind of request this server does not know")]
    Unknown,
    #[error("the login was refused")]
    LoginFailed,
    #[error("the login was cut off without an answer")]
    LoginAbandoned,
    #[error("a {0:?} request was made out of order in the login")]
    OutOfOrder(ApiKey),
    #[error("no login within the time allowed")]
    LoginTooSlow,
}

/// Serves `stream` until the client goes, something is wrong with it, or `stop` is cancelled.
pub(crate) async fn serve<S: AsyncRead + AsyncWrite + Unpin, F: Fs>(
    mut stream: S,
    ctx: &Context<F>,
    peer: Option<std::net::IpAddr>,
    stop: &CancellationToken,
) -> End {
    // What the broker is told of this connection: where it is from, for the group APIs that show members.
    let listener = peer.map_or_else(|| ctx.listener.clone(), |p| ctx.listener.seen_from(p));
    let mut buf = BytesMut::with_capacity(BUFFER);
    // A broker that wants a login starts every connection as a stranger, held to a small frame and a deadline.
    let mut session = ctx.auth.as_ref().map(|_| Session::new());
    let mut login_by = session
        .as_ref()
        .map(|_| Instant::now() + ctx.timeouts.pre_auth);
    let mut decoder = FrameDecoder::new(if session.is_some() {
        Trust::Anonymous
    } else {
        Trust::Authenticated
    });
    loop {
        let wait = (&ctx.timeouts, login_by);
        let frame = match frames::next_before(&mut stream, &mut buf, &mut decoder, wait, stop).await
        {
            Ok(frame) => frame,
            Err(end) => return end,
        };
        let ears = (ctx, &listener);
        if let Err(end) = exchange::exchange(&mut stream, ears, &mut session, frame, stop).await {
            return end;
        }
        if login_by.is_some() && session.as_ref().is_some_and(Session::is_authenticated) {
            decoder.set_authenticated();
            login_by = None;
            if let Some(who) = session.as_ref().and_then(Session::principal) {
                debug!(principal = %who, "logged in");
            }
        }
        if buf.is_empty() && buf.capacity() > KEEP {
            buf = BytesMut::with_capacity(BUFFER);
        }
    }
}

#[cfg(test)]
mod tests;
