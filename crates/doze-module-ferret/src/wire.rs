//! The few MongoDB commands converge needs, sent over FerretDB's own socket: one request at a time, each with a
//! deadline. The framing is `msg`'s.

use crate::bson::{Bson, Doc, Malformed};
use crate::msg::{Failure, MAX_MESSAGE, MIN_MESSAGE, frame, outcome, reply};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// One connection to FerretDB.
#[derive(Debug)]
pub(crate) struct Client {
    stream: doze_os::net::Stream,
    next: i32,
    deadline: Duration,
}

impl Client {
    /// A client on a connection to the gateway; each command must be answered within `deadline`.
    pub(crate) fn new(stream: doze_os::net::Stream, deadline: Duration) -> Client {
        Client {
            stream,
            next: 0,
            deadline,
        }
    }

    /// Runs `cmd` against database `db`; the reply when the server says `ok: 1`.
    pub(crate) async fn run(&mut self, db: &str, cmd: Doc) -> Result<Doc, Failure> {
        self.next = self.next.wrapping_add(1);
        let id = self.next;
        let msg = frame(id, &cmd.with("$db", db));
        let answer = tokio::time::timeout(self.deadline, self.exchange(&msg))
            .await
            .map_err(|_| Failure::Io(std::io::ErrorKind::TimedOut.into()))??;
        outcome(reply(&answer, id).map_err(Failure::Malformed)?)
    }

    async fn exchange(&mut self, msg: &[u8]) -> Result<Vec<u8>, Failure> {
        self.stream.write_all(msg).await.map_err(Failure::Io)?;
        let mut head = [0u8; 4];
        self.stream
            .read_exact(&mut head)
            .await
            .map_err(Failure::Io)?;
        let len = usize::try_from(i32::from_le_bytes(head)).unwrap_or(0);
        if !(MIN_MESSAGE..=MAX_MESSAGE).contains(&len) {
            return Err(Failure::Malformed(Malformed("reply length out of range")));
        }
        let mut whole = vec![0u8; len];
        let (h, rest) = whole.split_at_mut(4);
        h.copy_from_slice(&head);
        self.stream.read_exact(rest).await.map_err(Failure::Io)?;
        Ok(whole)
    }

    /// Every document of a cursor command's first batch and the batches after it (`listCollections`,
    /// `listIndexes`).
    #[allow(
        dead_code,
        reason = "tests/acceptance.rs reads what converge made with it, through this file"
    )]
    pub(crate) async fn all(
        &mut self,
        db: &str,
        cmd: Doc,
        coll: &str,
    ) -> Result<Vec<Doc>, Failure> {
        let mut r = self.run(db, cmd).await?;
        let mut out = Vec::new();
        let mut batch = "firstBatch";
        loop {
            let cursor = r.doc("cursor").cloned().unwrap_or_default();
            out.extend(
                cursor
                    .array(batch)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|d| match d {
                        Bson::Doc(d) => Some(d.clone()),
                        _ => None,
                    }),
            );
            match cursor.get("id") {
                Some(Bson::Int64(0) | Bson::Int32(0)) | None => return Ok(out),
                Some(id) => {
                    let more = Doc::new()
                        .with("getMore", id.clone())
                        .with("collection", coll);
                    r = self.run(db, more).await?;
                    batch = "nextBatch";
                }
            }
        }
    }
}
