//! Getting one request frame off a stream, within the time allowed.

use std::io;

use bytes::{Bytes, BytesMut};
use kafka_wire::frame::FrameDecoder;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::Timeouts;
use crate::connection::End;

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
