//! A RESP2 client over any byte stream (a Unix socket or loopback TCP, whichever backend the host granted): one
//! command, one reply, in order; or, after `MONITOR`, replies the server pushes unasked.

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

    /// A client that has authenticated with `password` (when there is one); a refused password is an error.
    pub async fn authenticated(stream: S, password: Option<&str>) -> io::Result<Self> {
        let mut c = Client::new(stream);
        if let Some(p) = password.filter(|p| !p.is_empty())
            && let Reply::Error(e) = c.call(&["AUTH", p]).await?
        {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, e));
        }
        Ok(c)
    }

    /// Sends `args` as one command and returns the server's reply (a `-ERR` reply is a reply, not an `Err`).
    pub async fn call(&mut self, args: &[&str]) -> io::Result<Reply> {
        self.stream.write_all(&encode(args)).await?;
        self.next_reply().await
    }

    /// The next reply the server sends, asked for or not.
    pub async fn next_reply(&mut self) -> io::Result<Reply> {
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

    /// The text of a command's reply; a `-ERR` reply becomes an error carrying the server's words.
    pub async fn text(&mut self, args: &[&str]) -> io::Result<String> {
        match self.call(args).await? {
            Reply::Error(e) => Err(io::Error::other(e)),
            Reply::Integer(n) => Ok(n.to_string()),
            r => r.text().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{args:?}: not text: {r:?}"),
                )
            }),
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

    #[tokio::test(flavor = "current_thread")]
    async fn a_refused_password_is_an_error_and_an_error_reply_is_not_text() {
        let (ours, mut server) = tokio::io::duplex(256);
        tokio::spawn(async move {
            let mut got = vec![0u8; 64];
            let _ = server.read(&mut got).await.unwrap();
            server
                .write_all(b"-WRONGPASS invalid password\r\n")
                .await
                .unwrap();
            let _ = server.read(&mut got).await.unwrap();
        });
        let e = Client::authenticated(ours, Some("nope")).await.unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        assert!(e.to_string().contains("WRONGPASS"));
    }
}
