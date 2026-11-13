//! An object's bytes leaving as a stream: slices of blob files read on blocking threads in pieces, so a large object is never held
//! whole and the runtime's thread is never held by a read.

use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use aws_wire::rest::{ByteStream, Download};
use bytes::Bytes;
use doze_fs::fs::{Fs, FsFile, RealFs};
use tokio::task::JoinHandle;

use crate::req::Slice;

/// The size of one read.
const PIECE: u64 = 256 * 1024;

type File = <RealFs as Fs>::File;

struct Pieces {
    queue: VecDeque<Slice>,
    reading: Option<JoinHandle<io::Result<Bytes>>>,
}

/// The stream over `slices`, in order.
pub(crate) fn stream(slices: Vec<Slice>) -> Download {
    Box::pin(Pieces {
        queue: slices.into(),
        reading: None,
    })
}

fn read(file: &Arc<File>, from: u64, len: usize) -> io::Result<Bytes> {
    let mut buf = vec![0u8; len];
    let mut at = 0;
    while at < len {
        let tail = buf.get_mut(at..).unwrap_or_default();
        let n = file.read_at(tail, from + at as u64)?;
        if n == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        at += n;
    }
    Ok(Bytes::from(buf))
}

impl Pieces {
    /// Starts the read of the next piece, if any is left.
    fn start(&mut self) -> bool {
        while let Some(front) = self.queue.front_mut() {
            if front.len == 0 {
                self.queue.pop_front();
                continue;
            }
            let take = front.len.min(PIECE);
            let (file, from) = (Arc::clone(&front.file), front.from);
            front.from += take;
            front.len -= take;
            let len = usize::try_from(take).unwrap_or(usize::MAX);
            self.reading = Some(tokio::task::spawn_blocking(move || read(&file, from, len)));
            return true;
        }
        false
    }
}

impl ByteStream for Pieces {
    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<io::Result<Bytes>>> {
        loop {
            if let Some(handle) = self.reading.as_mut() {
                let done = match Pin::new(handle).poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(done) => done,
                };
                self.reading = None;
                return Poll::Ready(Some(done.unwrap_or_else(|e| Err(io::Error::other(e)))));
            }
            if !self.start() {
                return Poll::Ready(None);
            }
        }
    }
}
