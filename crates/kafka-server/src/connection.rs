//! One connection: read a request, answer it, repeat.

use std::io;
use std::sync::Arc;

use bytes::BytesMut;
use kafka_auth::Authenticator;
use kafka_broker::{Broker, Caller, Listener};
use kafka_storage::fs::Fs;
use kafka_wire::WireError;
use kafka_wire::frame::{FrameDecoder, Trust};
use kafka_wire::messages::ApiKey;
use kafka_wire::profile::{Features, Profile};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::observer::Watch;
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
    /// Who is told of connections and requests, if anybody.
    pub(crate) observer: Option<Arc<dyn crate::Observer>>,
    /// Who may bend requests on purpose, if anybody.
    pub(crate) fault: Option<Arc<dyn crate::fault::FaultPlan>>,
    /// Where connection numbers come from (taken from only with an observer).
    pub(crate) numbers: crate::connections::Numbers,
}

/// What the exchange of one request has to hand: the server's shared parts, what the broker is told of the connection, who it
/// is, and the observer's view of it (none, in the usual case).
pub(crate) struct Ears<'a, F: Fs> {
    pub(crate) ctx: &'a Context<F>,
    pub(crate) listener: &'a Listener,
    pub(crate) caller: &'a Caller,
    pub(crate) watch: Option<&'a Watch>,
}

/// Why a connection ended. None of these texts holds anything a client sent.
#[derive(Debug, thiserror::Error)]
pub(crate) enum End {
    #[error("the client closed the connection")]
    PeerClosed,
    #[error("closed by a Lab rule")]
    Faulted,
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
    /// The kind of failure, in words of ours (`tls::kind`); never what the client sent.
    #[cfg(feature = "tls")]
    #[error("the TLS handshake failed: {0}")]
    Handshake(&'static str),
    #[cfg(feature = "tls")]
    #[error("the TLS handshake took too long")]
    HandshakeTooSlow,
}

/// Serves `stream` until the client goes, something is wrong with it, or `stop` is cancelled. `accepted` is when the
/// connection arrived: the login budget counts from there, so a TLS handshake that came first has used some of it.
pub(crate) async fn serve<S: AsyncRead + AsyncWrite + Unpin, F: Fs>(
    stream: &mut S,
    ctx: &Context<F>,
    peer: Option<std::net::SocketAddr>,
    accepted: Instant,
    stop: &CancellationToken,
) -> End {
    let watch = ctx.numbers.watch(ctx.observer.as_ref(), peer);
    let end = serve_loop(stream, ctx, peer, accepted, stop, watch.as_ref()).await;
    if let Some(w) = &watch {
        w.observer.closed(w.conn, &end.to_string());
    }
    end
}

async fn serve_loop<S: AsyncRead + AsyncWrite + Unpin, F: Fs>(
    stream: &mut S,
    ctx: &Context<F>,
    peer: Option<std::net::SocketAddr>,
    accepted: Instant,
    stop: &CancellationToken,
    watch: Option<&Watch>,
) -> End {
    let ip = peer.map(|p| p.ip());
    // What the broker is told of this connection: where it is from, for the group APIs that show members.
    let listener = peer.map_or_else(|| ctx.listener.clone(), |p| ctx.listener.seen_from(p.ip()));
    // And who it is: nobody in particular until a login says otherwise (and for good, if the server asks for none).
    let mut caller = Caller::anonymous(ip);
    let mut buf = BytesMut::with_capacity(BUFFER);
    // A broker that wants a login starts every connection as a stranger, held to a small frame and a deadline.
    let mut session = ctx.auth.as_ref().map(|_| Session::new());
    let mut login_by = session.as_ref().map(|_| accepted + ctx.timeouts.pre_auth);
    let mut decoder = FrameDecoder::new(if session.is_some() {
        Trust::Anonymous
    } else {
        Trust::Authenticated
    });
    loop {
        let wait = (&ctx.timeouts, login_by);
        let frame = match frames::next_before(stream, &mut buf, &mut decoder, wait, stop).await {
            Ok(frame) => frame,
            Err(end) => return end,
        };
        let ears = Ears {
            ctx,
            listener: &listener,
            caller: &caller,
            watch,
        };
        if let Err(end) = exchange::exchange(stream, ears, &mut session, frame, stop).await {
            return end;
        }
        if login_by.is_some() && session.as_ref().is_some_and(Session::is_authenticated) {
            decoder.set_authenticated();
            login_by = None;
            if let Some(who) = session.as_ref().and_then(Session::principal) {
                debug!(principal = %who, "logged in");
                caller = Caller::new(who.clone(), ip);
            }
        }
        if buf.is_empty() && buf.capacity() > KEEP {
            buf = BytesMut::with_capacity(BUFFER);
        }
    }
}

#[cfg(test)]
mod tests;
