//! One request, from its frame to the end of its answer.

use std::sync::Arc;
use std::time::Duration;

use bytes::{Buf, Bytes};
use kafka_broker::Outcome;
use kafka_storage::fs::Fs;
use kafka_wire::api::{Request, Response};
use kafka_wire::api_versions;
use kafka_wire::header::{self, RequestHead};
use kafka_wire::incoming::Incoming;
use kafka_wire::messages::ApiKey;
use kafka_wire::response::{self, Frame};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use super::{Context, Ears, End};
use crate::config::MOST_FAILED_LOGIN_DELAY;
use crate::fault::{Applied, Note};
use crate::observer::{Answer as Seen, Asked, Pending, Watch};
use crate::session::steps::{self, Fate};
use crate::session::{Session, Verdict};

mod lab;

/// How long the connection reads nothing more from the client once a request is answered: a client over its quota is
/// muted for the throttle time (KIP-219).
pub(super) type Mute = Option<Duration>;

/// What one request came to: an answer to write (and the observer's view of the request), if any, and the mute after it.
type Handled = (Option<(Answer, Option<Pending>)>, Mute);

/// An answer to write, and what happens to the connection after it.
struct Answer {
    id: i32,
    version: i16,
    response: Response,
    fate: Fate,
    /// The fault rule that made this answer or delayed it, for the observer.
    fault: Option<Arc<Note>>,
}
/// Reads the request in `frame`, answers it on `stream`, and says whether the connection goes on.
///
/// With a `session` the request is first judged on its header alone ([`Session::admit`]): a stranger's body
/// is decoded only for an API it may use. With a watch (an installed observer) every request that has a header is
/// told of twice, whatever becomes of it: an answer, no answer, or a close. Says how long to mute the connection after.
pub(super) async fn exchange<S: AsyncRead + AsyncWrite + Unpin, F: Fs>(
    stream: &mut S,
    ears: Ears<'_, F>,
    session: &mut Option<Session>,
    frame: Bytes,
    stop: &CancellationToken,
) -> Result<Mute, End> {
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
                let frame = seal(&answer, ears.watch.zip(pending))?;
                send(stream, ctx, frame).await?;
                if stays_open {
                    return Ok(None);
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
    let (answered, mute) = answer(&ears, session, incoming, frame_len, stop).await?;
    let Some((answer, pending)) = answered else {
        return Ok(mute);
    };
    let fate = answer.fate;
    // Observed (and so timed) before the failed-login delay, which is deliberate and not the broker's doing.
    let frame = seal(&answer, ears.watch.zip(pending))?;
    if fate == Fate::Failed {
        failed_login_delay(ctx, stop).await?;
    }
    send(stream, ctx, frame).await?;
    match fate {
        Fate::Open => Ok(mute),
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
        w.end(p, Seen::Closed(&end.to_string()), 0, None);
    }
    end
}

async fn answer<F: Fs>(
    ears: &Ears<'_, F>,
    session: &mut Option<Session>,
    incoming: Incoming,
    frame_len: usize,
    stop: &CancellationToken,
) -> Result<Handled, End> {
    let ctx = ears.ctx;
    match incoming {
        Incoming::ApiVersions { head, body } => {
            let pending = begin(ears, &head, Asked::ApiVersions(body.as_ref()), frame_len);
            let (note, pending) = lab::bend(ears, &head, pending, stop).await?;
            let (version, answer) =
                api_versions::answer(ctx.profile, ctx.features, head.version, body.as_ref());
            if answer.error_code == 0
                && let Some(s) = session.as_mut()
            {
                s.versions_answered();
            }
            let mut open = open(head.correlation_id, version, Response::ApiVersions(answer));
            open.fault = note;
            Ok((Some((open, pending)), None))
        }
        Incoming::Request { head, request } => {
            let pending = begin(ears, &head, Asked::Request(&request), frame_len);
            match request {
                Request::SaslHandshake(_) | Request::SaslAuthenticate(_) => {
                    match sasl(ctx, session, &head, request) {
                        Ok(answer) => Ok((Some((answer, pending)), None)),
                        Err(end) => Err(closed(ears.watch, pending, end)),
                    }
                }
                request => handled(ears, &head, request, pending, frame_len, stop).await,
            }
        }
        _ => Err(End::Unknown),
    }
}

/// A request for the broker: handed over, held to the client's quotas, answered, and observed. `size` is the request's
/// size on the wire, header and body.
async fn handled<F: Fs>(
    ears: &Ears<'_, F>,
    head: &RequestHead,
    request: Request,
    pending: Option<Pending>,
    size: usize,
    stop: &CancellationToken,
) -> Result<Handled, End> {
    let delay = match lab::plan(ears, head, &request) {
        lab::Plan::Serve(act) => act,
        lab::Plan::Answer(answer) => return Ok((Some((*answer, pending)), None)),
        lab::Plan::Silent(note) => {
            if let Some((w, p)) = ears.watch.zip(pending) {
                w.end(p, Seen::Nothing, 0, Some(&note));
            }
            return Ok((None, None));
        }
        lab::Plan::Close(note) => return Err(lab::dropped(ears.watch, pending, &note)),
    };
    // A handler may wait a long time (a long-poll Fetch): stopping the server does not wait for it.
    let served = tokio::select! {
        s = ears.ctx.broker.serve(ears.listener, ears.caller, head, request, size) => s,
        () = stop.cancelled() => return Err(closed(ears.watch, pending, End::Stopped)),
    };
    let mute = served.mute;
    // A rule that slows the request makes the answer wait, after the broker has made it.
    let note = match delay {
        Some(act) => {
            if let Applied::Delay(d) = act.effect
                && !lab::wait(d, stop).await
            {
                return Err(closed(ears.watch, pending, End::Stopped));
            }
            Some(act.note)
        }
        None => None,
    };
    match served.outcome {
        Outcome::Respond(response) => {
            let mut answer = open(head.correlation_id, head.version, response);
            answer.fault = note;
            Ok((Some((answer, pending)), mute))
        }
        Outcome::NoResponse => {
            if let Some((w, p)) = ears.watch.zip(pending) {
                w.end(p, Seen::Nothing, 0, note.as_deref());
            }
            Ok((None, mute))
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
            let (response, fate) = steps::handshake(session, auth, &r, head.version);
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
        fault: None,
    })
}

/// A raw SASL token, the frame after a SaslHandshake v0 (or after the server's first raw reply): judged, and the
/// mechanism's reply written back as a size-prefixed frame of its own. A refusal writes nothing: the connection is
/// closed, after the failed-login delay for a failed login (SEC-401). Raw frames have no header, so no observer
/// is told of them.
pub(super) async fn raw_token<S: AsyncRead + AsyncWrite + Unpin, F: Fs>(
    stream: &mut S,
    ctx: &Context<F>,
    session: &mut Session,
    token: Bytes,
    stop: &CancellationToken,
) -> Result<(), End> {
    let Some(auth) = ctx.auth.as_deref() else {
        return Err(End::LoginAbandoned);
    };
    match steps::raw(session, auth, &token) {
        Ok(reply) => {
            let len = u32::try_from(reply.len()).map_err(|_| End::LoginAbandoned)?;
            let mut frame = bytes::BytesMut::with_capacity(4 + reply.len());
            frame.extend_from_slice(&len.to_be_bytes());
            frame.extend_from_slice(&reply);
            send_bytes(stream, ctx, frame.freeze()).await
        }
        Err(steps::RawEnd::Failed) => {
            failed_login_delay(ctx, stop).await?;
            hang_up(stream, ctx, stop).await;
            Err(End::LoginFailed)
        }
        Err(steps::RawEnd::Hangup) => Err(End::LoginAbandoned),
    }
}

/// Writes `bytes` and flushes them, within the write timeout.
async fn send_bytes<S: AsyncWrite + Unpin, F: Fs>(
    stream: &mut S,
    ctx: &Context<F>,
    bytes: Bytes,
) -> Result<(), End> {
    let write = async {
        stream.write_all(&bytes).await?;
        stream.flush().await
    };
    match timeout(ctx.timeouts.write, write).await {
        Err(_) => Err(End::Slow),
        Ok(Err(e)) => Err(End::Io(e)),
        Ok(Ok(())) => Ok(()),
    }
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
        fault: None,
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
fn seal(answer: &Answer, seen: Option<(&Watch, Pending)>) -> Result<Frame, End> {
    let encoded = response::frame(answer.id, answer.version, &answer.response);
    if let Some((w, p)) = seen {
        match &encoded {
            Ok(frame) => w.end(
                p,
                Seen::Response(&answer.response),
                frame.remaining(),
                answer.fault.as_deref(),
            ),
            Err(e) => w.end(p, Seen::Closed(&e.to_string()), 0, answer.fault.as_deref()),
        }
    }
    encoded.map_err(End::Wire)
}

/// Writes `frame` and flushes it, within the write timeout.
///
/// A stream that writes vectored (a TCP socket, rustls) is given the frame's pieces together, so a Fetch's records go out
/// from the buffers the log was read into. The flush matters over TLS: rustls can still hold the end of the frame when the
/// last write returns, and nothing else would send it before the client's next request.
async fn send<S: AsyncWrite + Unpin, F: Fs>(
    stream: &mut S,
    ctx: &Context<F>,
    mut frame: Frame,
) -> Result<(), End> {
    let write = async {
        stream.write_all_buf(&mut frame).await?;
        stream.flush().await
    };
    match timeout(ctx.timeouts.write, write).await {
        Err(_) => Err(End::Slow),
        Ok(Err(e)) => Err(End::Io(e)),
        Ok(Ok(())) => Ok(()),
    }
}
