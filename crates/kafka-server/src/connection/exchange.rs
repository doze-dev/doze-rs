//! One request, from its frame to the end of its answer.

use bytes::Bytes;
use kafka_broker::{Caller, Listener, Outcome};
use kafka_storage::fs::Fs;
use kafka_wire::api::{Request, Response};
use kafka_wire::api_versions;
use kafka_wire::header::{self, RequestHead};
use kafka_wire::incoming::Incoming;
use kafka_wire::messages::ApiKey;
use kafka_wire::response;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use super::{Context, End};
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
/// is decoded only for an API it may use.
pub(super) async fn exchange<S: AsyncWrite + Unpin, F: Fs>(
    stream: &mut S,
    (ctx, listener, caller): (&Context<F>, &Listener, &Caller),
    session: &mut Option<Session>,
    frame: Bytes,
    stop: &CancellationToken,
) -> Result<(), End> {
    let (head, body) = header::split(frame).map_err(End::Wire)?;
    if let Some(verdict) = session.as_ref().map(|s| s.admit(head.key)) {
        match verdict {
            Verdict::Serve => {}
            Verdict::Close => return Err(End::OutOfOrder(head.key)),
            Verdict::IllegalState { stays_open } => {
                let answer = illegal_state(ctx, &head)?;
                write(stream, ctx, answer).await?;
                if stays_open {
                    return Ok(());
                }
                hang_up(stream, ctx).await;
                return Err(End::OutOfOrder(head.key));
            }
        }
    }
    let incoming = Incoming::decode(head, body, ctx.profile, ctx.features).map_err(End::Wire)?;
    let Some(answer) = answer(ctx, (listener, caller), session, incoming, stop).await? else {
        return Ok(());
    };
    let fate = answer.fate;
    if fate == Fate::Failed {
        failed_login_delay(ctx, stop).await?;
    }
    write(stream, ctx, answer).await?;
    match fate {
        Fate::Open => Ok(()),
        Fate::Closing | Fate::Failed => {
            hang_up(stream, ctx).await;
            Err(End::LoginFailed)
        }
    }
}

async fn answer<F: Fs>(
    ctx: &Context<F>,
    (listener, caller): (&Listener, &Caller),
    session: &mut Option<Session>,
    incoming: Incoming,
    stop: &CancellationToken,
) -> Result<Option<Answer>, End> {
    match incoming {
        Incoming::ApiVersions { head, body } => {
            let (version, answer) =
                api_versions::answer(ctx.profile, ctx.features, head.version, body.as_ref());
            if answer.error_code == 0
                && let Some(s) = session.as_mut()
            {
                s.versions_answered();
            }
            Ok(Some(open(
                head.correlation_id,
                version,
                Response::ApiVersions(answer),
            )))
        }
        Incoming::Request { head, request } => match request {
            Request::SaslHandshake(_) | Request::SaslAuthenticate(_) => {
                sasl(ctx, session, &head, request).map(Some)
            }
            request => {
                // A handler may wait a long time (a long-poll Fetch): stopping the server does not wait for it.
                let outcome = tokio::select! {
                    o = ctx.broker.handle(listener, caller, &head, request) => o,
                    () = stop.cancelled() => return Err(End::Stopped),
                };
                match outcome {
                    Outcome::Respond(response) => {
                        Ok(Some(open(head.correlation_id, head.version, response)))
                    }
                    Outcome::NoResponse => Ok(None),
                    _ => Err(End::Refused(head.key)),
                }
            }
        },
        _ => Err(End::Unknown),
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
    if ctx.timeouts.failed_login.is_zero() {
        return Ok(());
    }
    tokio::select! {
        () = tokio::time::sleep(ctx.timeouts.failed_login) => Ok(()),
        () = stop.cancelled() => Err(End::Stopped),
    }
}

/// Ends the stream cleanly after a refusal was written, so the client reads the answer and not a reset.
async fn hang_up<S: AsyncWrite + Unpin, F: Fs>(stream: &mut S, ctx: &Context<F>) {
    let _ = timeout(ctx.timeouts.write, stream.shutdown()).await;
}

async fn write<S: AsyncWrite + Unpin, F: Fs>(
    stream: &mut S,
    ctx: &Context<F>,
    answer: Answer,
) -> Result<(), End> {
    let bytes = response::encode(answer.id, answer.version, &answer.response).map_err(End::Wire)?;
    match timeout(ctx.timeouts.write, stream.write_all(&bytes)).await {
        Err(_) => Err(End::Slow),
        Ok(Err(e)) => Err(End::Io(e)),
        Ok(Ok(())) => Ok(()),
    }
}
