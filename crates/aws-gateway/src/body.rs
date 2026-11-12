//! The body of an answer: whole, or streamed from the service.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use aws_wire::rest::Download;
use bytes::Bytes;
use hyper::body::{Body, Frame, SizeHint};

/// An answer's body.
pub(crate) enum RespBody {
    /// Sent whole (taken on the first poll).
    Whole(Option<Bytes>),
    /// Pulled from the service piece by piece.
    Stream(Download),
}

impl RespBody {
    pub(crate) fn whole(bytes: Vec<u8>) -> Self {
        Self::Whole(Some(Bytes::from(bytes)))
    }
}

impl Body for RespBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        match self.get_mut() {
            Self::Whole(bytes) => Poll::Ready(
                bytes
                    .take()
                    .filter(|b| !b.is_empty())
                    .map(|b| Ok(Frame::data(b))),
            ),
            Self::Stream(stream) => stream
                .as_mut()
                .poll_next(cx)
                .map(|piece| piece.map(|r| r.map(Frame::data))),
        }
    }

    fn is_end_stream(&self) -> bool {
        matches!(self, Self::Whole(None))
    }

    fn size_hint(&self) -> SizeHint {
        match self {
            Self::Whole(Some(b)) => SizeHint::with_exact(b.len() as u64),
            Self::Whole(None) => SizeHint::with_exact(0),
            Self::Stream(_) => SizeHint::default(),
        }
    }
}

impl std::fmt::Debug for RespBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RespBody")
    }
}
