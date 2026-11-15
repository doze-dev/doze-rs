//! A request in flight: [`Pending`] for one answer, [`Stream`] for a subscription and the other streams. Both wait on
//! an `async_channel` receiver, from a plain thread (with a deadline) or from async code.

use super::connection::Connection;
use super::wait::block_on;
use crate::{Error, Frame, Response};
use async_channel::{Receiver, RecvError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A unary request waiting for its response.
#[derive(Debug)]
pub struct Pending {
    id: u64,
    op: String,
    rx: Receiver<Frame>,
    conn: Connection,
}

impl Pending {
    pub(crate) fn new(id: u64, op: String, rx: Receiver<Frame>, conn: Connection) -> Pending {
        Pending { id, op, rx, conn }
    }

    /// The request's id.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Waits for the response, at most `timeout` (`None`: for ever).
    pub fn wait(self, timeout: Option<Duration>) -> Result<Response, Error> {
        match block_on(self.rx.recv(), timeout.map(|t| Instant::now() + t)) {
            Some(r) => self.finish(r),
            None => Err(Error::Timeout(timeout.unwrap_or_default())),
        }
    }

    /// Waits for the response from async code.
    pub async fn response(self) -> Result<Response, Error> {
        let r = self.rx.recv().await;
        self.finish(r)
    }

    fn finish(&self, r: Result<Frame, RecvError>) -> Result<Response, Error> {
        match r {
            Err(_) => Err(Error::Closed(self.conn.shared().why())),
            Ok(Frame { response: None, .. }) => Err(Error::NoResponse(self.op.clone())),
            Ok(Frame {
                response: Some(r), ..
            }) if !r.error.is_empty() => Err(Error::from_response(&r)),
            Ok(Frame {
                response: Some(r), ..
            }) => Ok(r),
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        // A late answer is dropped instead of queueing for nobody.
        self.conn.shared().forget(self.id);
    }
}

/// What a stream produced.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// A topic frame.
    Frame(Box<Frame>),
    /// The daemon ended the stream: after a cancel or by itself (`None`), or because it failed.
    End(Option<String>),
    /// The connection went away first.
    Lost(String),
}

/// An acknowledged stream. Dropping it cancels it on the daemon's side.
#[derive(Debug)]
pub struct Stream {
    id: u64,
    rx: Receiver<Frame>,
    conn: Connection,
    ended: AtomicBool,
}

impl Stream {
    pub(crate) fn new(id: u64, rx: Receiver<Frame>, conn: Connection) -> Stream {
        Stream {
            id,
            rx,
            conn,
            ended: AtomicBool::new(false),
        }
    }

    /// The request's id, which every frame of the stream carries.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The raw frames, for a consumer that selects over several channels. The stream's last frame has `end` set;
    /// after it, or when the connection goes, the channel is closed.
    pub fn frames(&self) -> &Receiver<Frame> {
        &self.rx
    }

    /// The next event, waiting as long as it takes.
    pub fn next_blocking(&self) -> StreamEvent {
        match block_on(self.rx.recv(), None) {
            Some(r) => self.event(r),
            None => StreamEvent::Lost("the wait ended".to_owned()),
        }
    }

    /// The next event, or `None` if nothing came within `timeout`.
    pub fn next_timeout(&self, timeout: Duration) -> Option<StreamEvent> {
        block_on(self.rx.recv(), Some(Instant::now() + timeout)).map(|r| self.event(r))
    }

    /// The next event, from async code.
    pub async fn next(&self) -> StreamEvent {
        let r = self.rx.recv().await;
        self.event(r)
    }

    /// Turns what the channel gave into an event.
    pub fn event(&self, r: Result<Frame, RecvError>) -> StreamEvent {
        match r {
            Ok(f) if f.end => {
                self.ended.store(true, Ordering::Relaxed);
                StreamEvent::End(Some(f.error).filter(|e| !e.is_empty()))
            }
            Ok(f) => StreamEvent::Frame(Box::new(f)),
            Err(_) => {
                self.ended.store(true, Ordering::Relaxed);
                StreamEvent::Lost(self.conn.shared().why())
            }
        }
    }

    /// Asks the daemon to end the stream and stops listening. The daemon stops sampling or following for it.
    pub fn cancel(self) {
        drop(self);
    }

    /// The connection the stream runs on.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if !self.ended.load(Ordering::Relaxed) {
            let _ = self.conn.cancel(self.id);
        }
        self.conn.shared().forget(self.id);
    }
}
