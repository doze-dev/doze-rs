//! A RESP2 client over any byte stream (a Unix socket or loopback TCP, whichever backend the host granted): one
//! command, one reply, in order.

use crate::reply::{Reply, encode, parse};
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// A connection to a Redis-protocol server.
#[derive(Debug)]
pub struct Client<S> {
    stream: S,
    buf: Vec<u8>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Client<S> {
    pub fn new(stream: S) -> Self {
        Client {
            stream,
            buf: Vec::new(),
        }
    }

    /// Sends `args` as one command and returns the server's reply (a `-ERR` reply is a reply, not an `Err`).
    pub async fn call(&mut self, args: &[&str]) -> io::Result<Reply> {
        self.stream.write_all(&encode(args)).await?;
        loop {
            let parsed =
                parse(&self.buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if let Some((reply, used)) = parsed {
                self.buf.drain(..used);
                return Ok(reply);
            }
            let mut chunk = [0u8; 4096];
            let n = self.stream.read(&mut chunk).await?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the server closed the connection before replying",
                ));
            }
            self.buf
                .extend_from_slice(chunk.get(..n).unwrap_or_default());
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn a_reply_split_across_reads_is_put_back_together() {
        let (ours, mut server) = tokio::io::duplex(8);
        let serve = tokio::spawn(async move {
            let mut got = vec![0u8; 14];
            server.read_exact(&mut got).await.unwrap();
            assert_eq!(got, b"*1\r\n$4\r\nPING\r\n");
            for piece in [&b"$11\r\nhello"[..], b" world", b"\r\n"] {
                server.write_all(piece).await.unwrap();
            }
            server
        });
        let mut c = Client::new(ours);
        let r = c.call(&["PING"]).await.unwrap();
        assert_eq!(r.text().as_deref(), Some("hello world"));
        drop(serve.await.unwrap());
        assert!(
            c.call(&["PING"]).await.is_err(),
            "a closed server is an error"
        );
    }
}
