//! The receiving end of a stream: items under credit; dropping it before the end cancels the call.

use super::{Peer, internal};
use crate::msg::{self, error::Error};
use tokio::sync::mpsc;

/// The receiving end of a stream.
#[derive(Debug)]
pub struct Stream {
    pub(super) id: u32,
    pub(super) rx: mpsc::UnboundedReceiver<Result<Vec<u8>, Error>>,
    pub(super) peer: Peer,
    pub(super) done: bool,
}

impl Stream {
    /// The next item, `None` at the end, or the error that ended it.
    pub async fn next<T: for<'b> minicbor::Decode<'b, ()>>(&mut self) -> Option<Result<T, Error>> {
        let item = self.rx.recv().await;
        let Some(item) = item else {
            self.done = true;
            return None;
        };
        self.peer.inner.consumed(self.id, self.rx.len());
        Some(item.and_then(|b| msg::decode(&b).map_err(internal)))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if !self.done {
            self.peer.cancel(self.id);
        }
    }
}
