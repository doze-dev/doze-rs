//! One connection: read a request, answer it, repeat.

use std::io;
use std::sync::Arc;

use bytes::BytesMut;
use kafka_broker::{Broker, Listener, Outcome};
use kafka_storage::fs::Fs;
use kafka_wire::WireError;
use kafka_wire::api::Response;
use kafka_wire::api_versions;
use kafka_wire::frame::{FrameDecoder, Trust};
use kafka_wire::header;
use kafka_wire::incoming::Incoming;
use kafka_wire::messages::ApiKey;
use kafka_wire::profile::{Features, Profile};
use kafka_wire::response;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::{Timeouts, frames};

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
}

/// Why a connection ended.
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
}

/// Serves `stream` until the client goes, something is wrong with it, or `stop` is cancelled.
pub(crate) async fn serve<S: AsyncRead + AsyncWrite + Unpin, F: Fs>(
    mut stream: S,
    ctx: &Context<F>,
    stop: &CancellationToken,
) -> End {
    let mut buf = BytesMut::with_capacity(BUFFER);
    let mut decoder = FrameDecoder::new(Trust::Authenticated);
    loop {
        let frame =
            match frames::next(&mut stream, &mut buf, &mut decoder, &ctx.timeouts, stop).await {
                Ok(frame) => frame,
                Err(end) => return end,
            };
        if let Err(end) = exchange(&mut stream, ctx, frame, stop).await {
            return end;
        }
        if buf.is_empty() && buf.capacity() > KEEP {
            buf = BytesMut::with_capacity(BUFFER);
        }
    }
}

/// One request, from its frame to the end of its answer.
async fn exchange<S: AsyncWrite + Unpin, F: Fs>(
    stream: &mut S,
    ctx: &Context<F>,
    frame: bytes::Bytes,
    stop: &CancellationToken,
) -> Result<(), End> {
    let (head, body) = header::split(frame).map_err(End::Wire)?;
    let incoming = Incoming::decode(head, body, ctx.profile, ctx.features).map_err(End::Wire)?;
    let (id, version, response) = match incoming {
        Incoming::ApiVersions { head, body } => {
            let (version, answer) =
                api_versions::answer(ctx.profile, ctx.features, head.version, body.as_ref());
            (head.correlation_id, version, Response::ApiVersions(answer))
        }
        Incoming::Request { head, request } => {
            // A handler may wait a long time (a long-poll Fetch): stopping the server does not wait for it.
            let outcome = tokio::select! {
                o = ctx.broker.handle(&ctx.listener, &head, request) => o,
                () = stop.cancelled() => return Err(End::Stopped),
            };
            match outcome {
                Outcome::Respond(response) => (head.correlation_id, head.version, response),
                Outcome::NoResponse => return Ok(()),
                _ => return Err(End::Refused(head.key)),
            }
        }
        _ => return Err(End::Unknown),
    };
    let bytes = response::encode(id, version, &response).map_err(End::Wire)?;
    match timeout(ctx.timeouts.write, stream.write_all(&bytes)).await {
        Err(_) => Err(End::Slow),
        Ok(Err(e)) => Err(End::Io(e)),
        Ok(Ok(())) => Ok(()),
    }
}

#[cfg(test)]
mod tests;
