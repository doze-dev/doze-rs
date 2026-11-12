//! Getting one request frame off a stream, within the time allowed.

use std::io;

use bytes::{BufMut, Bytes, BytesMut};
use kafka_wire::frame::FrameDecoder;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::Timeouts;
use crate::connection::End;
use crate::pool::{self, POOL};

/// [`next`], but only until `login_by` if there is one: a connection that has not logged in by then is
/// `End::LoginTooSlow`, whatever it is doing. The budget is for the whole login, counted from the accept, so
/// a stranger who sends a byte every few seconds does not hold the connection.
pub(crate) async fn next_before<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
    decoder: &mut FrameDecoder,
    (timeouts, login_by): (&Timeouts, Option<Instant>),
    stop: &CancellationToken,
) -> Result<Bytes, End> {
    let frame = next(stream, buf, decoder, timeouts, stop);
    match login_by {
        None => frame.await,
        Some(by) => timeout_at(by, frame)
            .await
            .unwrap_or(Err(End::LoginTooSlow)),
    }
}

/// The next complete frame. `buf` carries what was read past the last frame (a client may send its next request
/// before it has the answer to this one).
///
/// The idle limit applies while nothing of a request has arrived; once something has, the whole frame has
/// `timeouts.frame` to arrive, counted from the first byte and not restarted by each trickle.
pub(crate) async fn next<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
    decoder: &mut FrameDecoder,
    timeouts: &Timeouts,
    stop: &CancellationToken,
) -> Result<Bytes, End> {
    // Bytes already in hand mean a request has begun.
    let mut deadline = (!buf.is_empty()).then(|| Instant::now() + timeouts.frame);
    loop {
        if let Some(len) = decoder.length(buf).map_err(End::Wire)?
            && into_pool(decoder, len as usize)
            && buf.len() < 4 + len as usize
        {
            // A big frame is read straight into a buffer that is kept for the next one.
            let until = deadline.unwrap_or_else(|| Instant::now() + timeouts.frame);
            return pooled(stream, buf, len as usize, until, stop).await;
        }
        if let Some(frame) = decoder.decode(buf).map_err(End::Wire)? {
            return Ok(frame);
        }
        let (until, on_timeout) = match deadline {
            Some(d) => (d, End::Slow),
            None => (Instant::now() + timeouts.idle, End::Idle),
        };
        let read = tokio::select! {
            r = timeout_at(until, stream.read_buf(buf)) => r,
            () = stop.cancelled() => return Err(End::Stopped),
        };
        match read {
            Err(_) => return Err(on_timeout),
            Ok(Err(e)) => return Err(End::Io(e)),
            Ok(Ok(0)) if buf.is_empty() => return Err(End::PeerClosed),
            Ok(Ok(0)) => return Err(End::Io(io::ErrorKind::UnexpectedEof.into())),
            Ok(Ok(_)) => {
                deadline.get_or_insert_with(|| Instant::now() + timeouts.frame);
            }
        }
    }
}

/// Whether a frame of `len` bytes is read into a pooled buffer: a big one, from a peer that has logged in. What a
/// stranger sends is never kept for the next frame (SEC-194).
fn into_pool(decoder: &FrameDecoder, len: usize) -> bool {
    decoder.is_authenticated() && pool::pooled(len)
}

/// A frame of `len` bytes whose length prefix and first bytes are in `buf`, read into a pooled buffer.
async fn pooled<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
    len: usize,
    until: Instant,
    stop: &CancellationToken,
) -> Result<Bytes, End> {
    let mut frame = POOL.take(len);
    frame.extend_from_slice(buf.get(4..).unwrap_or_default());
    buf.clear();
    while frame.len() < len {
        let missing = len - frame.len();
        let mut room = (&mut frame).limit(missing);
        let read = tokio::select! {
            r = timeout_at(until, stream.read_buf(&mut room)) => r,
            () = stop.cancelled() => return Err(End::Stopped),
        };
        match read {
            Err(_) => return Err(End::Slow),
            Ok(Err(e)) => return Err(End::Io(e)),
            Ok(Ok(0)) => return Err(End::Io(io::ErrorKind::UnexpectedEof.into())),
            Ok(Ok(_)) => {}
        }
    }
    Ok(POOL.share(frame))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kafka_wire::frame::Trust;

    #[test]
    fn only_a_peer_that_has_logged_in_gets_a_pooled_buffer() {
        // SEC-194: a stranger's frame, at the size the pool would take, is read as any other and freed.
        let mut d = FrameDecoder::new(Trust::Anonymous);
        for len in [pool::MIN, pool::MIN + 1, 1 << 20] {
            assert!(!into_pool(&d, len), "{len} from a stranger");
        }
        d.set_authenticated();
        assert!(into_pool(&d, pool::MIN) && into_pool(&d, 1 << 20));
        assert!(!into_pool(&d, pool::MIN - 1) && !into_pool(&d, pool::MAX + 1));
    }
}
