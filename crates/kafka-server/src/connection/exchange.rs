//! One request, from its frame to the end of its answer.

use std::time::Duration;

use bytes::Bytes;
use kafka_broker::Outcome;
use kafka_storage::fs::Fs;
use kafka_wire::api::{Request, Response};
use kafka_wire::api_versions;
use kafka_wire::header::{self, RequestHead};
use kafka_wire::incoming::Incoming;
use kafka_wire::messages::ApiKey;
use kafka_wire::response;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use super::{Context, Ears, End};
use crate::config::MOST_FAILED_LOGIN_DELAY;
use crate::observer::{Answer as Seen, Asked, Pending, Watch};
use crate::session::steps::{self, Fate};
use crate::session::{Session, Verdict};

/// An answer to write, and what happens to the connection after it.
struct Answer {
    id: i32,
    version: i16,
    response: Response,
    fate: Fate,
}
/// Reads the request in `frame`, answers it on `stream`, and says whether the connection goes on.
///
/// With a `session` the request is first judged on its header alone ([`Session::admit`]): a stranger's body
/// is decoded only for an API it may use. With a watch (an installed observer) every request that has a header is
/// told of twice, whatever becomes of it: an answer, no answer, or a close.
pub(super) async fn exchange<S: AsyncRead + AsyncWrite + Unpin, F: Fs>(
    stream: &mut S,
    ears: Ears<'_, F>,
    session: &mut Option<Session>,
    frame: Bytes,
    stop: &CancellationToken,
) -> Result<(), End> {
    let ctx = ears.ctx;
    let frame_len = frame.len();
    let (head, body) = header::split(frame).map_err(End::Wire)?;
    if let Some(verdict) = session.as_ref().map(|s| s.admit(head.key)) {
        match verdict {
            Verdict::Serve => {}
            Verdict::Close => {
                let pending = begin(&ears, &head, Asked::Undecoded, frame_len);
                return Err(closed(ears.watch, pending, End::OutOfOrder(head.key)));
            }
            Verdict::IllegalState { stays_open } => {
                let pending = begin(&ears, &head, Asked::Undecoded, frame_len);
                let answer = match illegal_state(ctx, &head) {
                    Ok(answer) => answer,
                    Err(end) => return Err(closed(ears.watch, pending, end)),
                };
                let bytes = seal(&answer, ears.watch.zip(pending))?;
                send(stream, ctx, bytes).await?;
                if stays_open {
                    return Ok(());
                }
                hang_up(stream, ctx, stop).await;
                return Err(End::OutOfOrder(head.key));
            }
        }
    }
    let kept = ears.watch.map(|_| head.clone());
    let incoming = match Incoming::decode(head, body, ctx.profile, ctx.features) {
        Ok(incoming) => incoming,
        Err(e) => {
            let pending = kept.and_then(|h| begin(&ears, &h, Asked::Undecoded, frame_len));
            return Err(closed(ears.watch, pending, End::Wire(e)));
        }
    };
    let Some((answer, pending)) = answer(&ears, session, incoming, frame_len, stop).await? else {
        return Ok(());
    };
    let fate = answer.fate;
    // Observed (and so timed) before the failed-login delay, which is deliberate and not the broker's doing.
    let bytes = seal(&answer, ears.watch.zip(pending))?;
    if fate == Fate::Failed {
        failed_login_delay(ctx, stop).await?;
    }
    send(stream, ctx, bytes).await?;
    match fate {
        Fate::Open => Ok(()),
        Fate::Closing | Fate::Failed => {
            hang_up(stream, ctx, stop).await;
            Err(End::LoginFailed)
        }
    }
}

/// Tells the watch, if there is one, that a request begins.
fn begin<F: Fs>(
    ears: &Ears<'_, F>,
    head: &RequestHead,
    asked: Asked<'_>,
    frame_len: usize,
) -> Option<Pending> {
    ears.watch
        .map(|w| w.begin(ears.caller.principal(), head, asked, frame_len))
}

/// Ends an observed request that the connection did not answer, and hands the reason back to be returned.
fn closed(watch: Option<&Watch>, pending: Option<Pending>, end: End) -> End {
    if let Some((w, p)) = watch.zip(pending) {
        w.end(p, Seen::Closed(&end.to_string()), 0);
    }
    end
}

async fn answer<F: Fs>(
    ears: &Ears<'_, F>,
    session: &mut Option<Session>,
    incoming: Incoming,
    frame_len: usize,
    stop: &CancellationToken,
) -> Result<Option<(Answer, Option<Pending>)>, End> {
    let ctx = ears.ctx;
    match incoming {
        Incoming::ApiVersions { head, body } => {
            let pending = begin(ears, &head, Asked::ApiVersions(body.as_ref()), frame_len);
            let (version, answer) =
                api_versions::answer(ctx.profile, ctx.features, head.version, body.as_ref());
            if answer.error_code == 0
                && let Some(s) = session.as_mut()
            {
                s.versions_answered();
            }
            let open = open(head.correlation_id, version, Response::ApiVersions(answer));
            Ok(Some((open, pending)))
        }
        Incoming::Request { head, request } => {
            let pending = begin(ears, &head, Asked::Request(&request), frame_len);
            match request {
                Request::SaslHandshake(_) | Request::SaslAuthenticate(_) => {
                    match sasl(ctx, session, &head, request) {
                        Ok(answer) => Ok(Some((answer, pending))),
                        Err(end) => Err(closed(ears.watch, pending, end)),
                    }
                }
                request => handled(ears, &head, request, pending, stop).await,
            }
        }
        _ => Err(End::Unknown),
    }
}

/// A request for the broker: handed over, answered, and observed.
async fn handled<F: Fs>(
    ears: &Ears<'_, F>,
    head: &RequestHead,
    request: Request,
    pending: Option<Pending>,
    stop: &CancellationToken,
) -> Result<Option<(Answer, Option<Pending>)>, End> {
    // A handler may wait a long time (a long-poll Fetch): stopping the server does not wait for it.
    let outcome = tokio::select! {
        o = ears.ctx.broker.handle(ears.listener, ears.caller, head, request) => o,
        () = stop.cancelled() => return Err(closed(ears.watch, pending, End::Stopped)),
    };
    match outcome {
        Outcome::Respond(response) => Ok(Some((
            open(head.correlation_id, head.version, response),
            pending,
        ))),
        Outcome::NoResponse => {
            if let Some((w, p)) = ears.watch.zip(pending) {
                w.end(p, Seen::Nothing, 0);
            }
            Ok(None)
        }
        _ => Err(closed(ears.watch, pending, End::Refused(head.key))),
    }
}

/// A SaslHandshake or SaslAuthenticate, answered by the server and not the broker.
fn sasl<F: Fs>(
    ctx: &Context<F>,
    session: &mut Option<Session>,
    head: &RequestHead,
    request: Request,
) -> Result<Answer, End> {
    let (Some(session), Some(auth)) = (session.as_mut(), ctx.auth.as_deref()) else {
        // Not advertised without SASL, so not decoded: this is a request the server never offered.
        return Err(End::Refused(head.key));
    };
    let (response, fate) = match request {
        Request::SaslHandshake(r) => {
            let (response, fate) = steps::handshake(session, auth, &r);
            (Response::SaslHandshake(response), fate)
        }
        Request::SaslAuthenticate(r) => {
            let (response, fate) = steps::authenticate(session, auth, &r)
                .map_err(|steps::Hangup| End::LoginAbandoned)?;
            (Response::SaslAuthenticate(response), fate)
        }
        _ => return Err(End::Refused(head.key)),
    };
    Ok(Answer {
        id: head.correlation_id,
        version: head.version,
        response,
        fate,
    })
}

/// The ILLEGAL_SASL_STATE answer for a request that was not decoded, in the shape of its API.
fn illegal_state<F: Fs>(ctx: &Context<F>, head: &RequestHead) -> Result<Answer, End> {
    let served = |r: Response| {
        ctx.profile
            .check(head.key, head.version, ctx.features)
            .map_err(End::Wire)
            .map(|()| open(head.correlation_id, head.version, r))
    };
    match head.key {
        ApiKey::ApiVersions => {
            let (version, answer) = api_versions::illegal_state(ctx.profile, head.version);
            Ok(open(
                head.correlation_id,
                version,
                Response::ApiVersions(answer),
            ))
        }
        ApiKey::SaslHandshake => served(Response::SaslHandshake(steps::illegal_handshake())),
        ApiKey::SaslAuthenticate => served(Response::SaslAuthenticate(steps::after_login())),
        key => Err(End::OutOfOrder(key)),
    }
}

fn open(id: i32, version: i16, response: Response) -> Answer {
    Answer {
        id,
        version,
        response,
        fate: Fate::Open,
    }
}

/// Waits the failed-login delay before a refusal is answered (SEC-192); a stop does not wait for it.
async fn failed_login_delay<F: Fs>(ctx: &Context<F>, stop: &CancellationToken) -> Result<(), End> {
    let wait = ctx.timeouts.failed_login.min(MOST_FAILED_LOGIN_DELAY);
    if wait.is_zero() {
        return Ok(());
    }
    tokio::select! {
        () = tokio::time::sleep(wait) => Ok(()),
        () = stop.cancelled() => Err(End::Stopped),
    }
}

/// How long a refused client has to close its side after the server has shut down its own.
const LINGER: Duration = Duration::from_millis(250);

/// Ends the stream cleanly after a refusal was written, so the client reads the answer and not a reset (SEC-195).
///
/// The server's side is shut down first, so the client sees the end at once. Then what the client sent behind the
/// refusal is read and thrown away until it closes its side or [`LINGER`] is over: closing a socket that still holds
/// unread bytes sends a reset, which can discard an answer the client has not yet read. A stop ends the wait at once.
async fn hang_up<S: AsyncRead + AsyncWrite + Unpin, F: Fs>(
    stream: &mut S,
    ctx: &Context<F>,
    stop: &CancellationToken,
) {
    tokio::select! {
        () = linger(stream, ctx) => {}
        () = stop.cancelled() => {}
    }
}

async fn linger<S: AsyncRead + AsyncWrite + Unpin, F: Fs>(stream: &mut S, ctx: &Context<F>) {
    if !matches!(
        timeout(ctx.timeouts.write, stream.shutdown()).await,
        Ok(Ok(()))
    ) {
        return;
    }
    let mut sink = [0u8; 4096];
    let drain = async { while matches!(stream.read(&mut sink).await, Ok(n) if n > 0) {} };
    let _ = timeout(LINGER, drain).await;
}

/// Encodes an answer, telling the watch (if any) how the request ended and how big its answer is.
fn seal(answer: &Answer, seen: Option<(&Watch, Pending)>) -> Result<Bytes, End> {
    let encoded = response::encode(answer.id, answer.version, &answer.response);
    if let Some((w, p)) = seen {
        match &encoded {
            Ok(bytes) => w.end(p, Seen::Response(&answer.response), bytes.len()),
            Err(e) => w.end(p, Seen::Closed(&e.to_string()), 0),
        }
    }
    encoded.map_err(End::Wire)
}

async fn send<S: AsyncWrite + Unpin, F: Fs>(
    stream: &mut S,
    ctx: &Context<F>,
    bytes: Bytes,
) -> Result<(), End> {
    match timeout(ctx.timeouts.write, stream.write_all(&bytes)).await {
        Err(_) => Err(End::Slow),
        Ok(Err(e)) => Err(End::Io(e)),
        Ok(Ok(())) => Ok(()),
    }
}
