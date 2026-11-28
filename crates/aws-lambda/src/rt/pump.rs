//! What a function process writes: its stdout and stderr read line by line by one task, into a bounded hold the invocation takes them
//! from. A line is cut at CloudWatch's event size, bytes that are not UTF-8 are replaced, and when a process prints more than the hold
//! keeps between two invocations the oldest lines go (the next invocation's log says how many).

use std::collections::VecDeque;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};

use super::worker::Line;

/// The most bytes of output kept for an invocation that has not taken them.
pub(crate) const HELD_BYTES: usize = 1024 * 1024;
/// The longest line: CloudWatch Logs' event size (256 KiB, of which 26 bytes are its own).
pub(crate) const LINE_MAX: usize = 256 * 1024 - 26;
/// How long the rest of the output is read for once the process has ended (something it started may hold the pipe open).
const TAIL: Duration = Duration::from_millis(100);

/// The lines a process wrote that no invocation has taken yet.
#[derive(Debug, Default)]
pub(crate) struct Held {
    lines: VecDeque<Line>,
    bytes: usize,
    dropped: u64,
}

impl Held {
    /// Keeps a line, letting the oldest go when the hold is full.
    pub(crate) fn push(&mut self, line: Line) {
        self.bytes += line.1.len();
        self.lines.push_back(line);
        while self.bytes > HELD_BYTES {
            let Some(old) = self.lines.pop_front() else {
                break;
            };
            self.bytes -= old.1.len();
            self.dropped += 1;
        }
    }

    /// Every line kept, oldest first; a note first when some were let go.
    pub(crate) fn take(&mut self) -> Vec<Line> {
        let mut out = Vec::with_capacity(self.lines.len() + 1);
        if self.dropped > 0 {
            let at = self.lines.front().map_or(0, |l| l.0);
            out.push((
                at,
                format!(
                    "doze-aws: {} earlier lines were dropped: the process printed more than {HELD_BYTES} bytes before an invocation took them",
                    self.dropped
                ),
            ));
        }
        out.extend(self.lines.drain(..));
        self.bytes = 0;
        self.dropped = 0;
        out
    }
}

/// One pipe, cut into lines.
pub(crate) struct Lines<R> {
    pipe: R,
    buf: Vec<u8>,
    pub(crate) done: bool,
}

impl<R: AsyncRead + Unpin> Lines<R> {
    pub(crate) fn new(pipe: R) -> Self {
        Self {
            pipe,
            buf: Vec::new(),
            done: false,
        }
    }

    /// Reads what is there. Cancel-safe: when another branch of a `select!` wins, nothing was read.
    pub(crate) async fn fill(&mut self) {
        let mut chunk = [0_u8; 8192];
        match self.pipe.read(&mut chunk).await {
            Ok(n) if n > 0 => self
                .buf
                .extend_from_slice(chunk.get(..n).unwrap_or_default()),
            _ => self.done = true,
        }
    }

    /// The next whole line read; at the end, what is left.
    pub(crate) fn line(&mut self) -> Option<String> {
        let cut = match self.buf.iter().position(|b| *b == b'\n') {
            Some(i) if i <= LINE_MAX => Some((i, 1)),
            _ if self.buf.len() >= LINE_MAX => Some((LINE_MAX, 0)),
            _ if self.done && !self.buf.is_empty() => Some((self.buf.len(), 0)),
            _ => None,
        }?;
        let (at, skip) = cut;
        let mut text: Vec<u8> = self.buf.drain(..at + skip).collect();
        text.truncate(at);
        if text.last() == Some(&b'\r') {
            text.pop();
        }
        if self.buf.is_empty() && self.buf.capacity() > 64 * 1024 {
            self.buf = Vec::new();
        }
        Some(String::from_utf8_lossy(&text).into_owned())
    }
}

/// Reads both pipes until both end, or until a moment after `ended` resolves; each line goes to `keep`.
pub(crate) async fn pump<O, E>(
    mut out: Lines<O>,
    mut err: Lines<E>,
    ended: impl std::future::Future<Output = ()>,
    mut keep: impl FnMut(String),
) where
    O: AsyncRead + Unpin,
    E: AsyncRead + Unpin,
{
    tokio::pin!(ended);
    let mut stop_at: Option<tokio::time::Instant> = None;
    loop {
        while let Some(line) = out.line() {
            keep(line);
        }
        while let Some(line) = err.line() {
            keep(line);
        }
        if out.done && err.done {
            return;
        }
        let tail = async {
            match stop_at {
                Some(t) => tokio::time::sleep_until(t).await,
                None => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            () = out.fill(), if !out.done => {}
            () = err.fill(), if !err.done => {}
            () = &mut ended, if stop_at.is_none() => stop_at = Some(tokio::time::Instant::now() + TAIL),
            () = tail => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hold_lets_the_oldest_lines_go_and_says_so() {
        let mut h = Held::default();
        let big = "x".repeat(HELD_BYTES / 2);
        for i in 0..3 {
            h.push((i, big.clone()));
        }
        let got = h.take();
        assert_eq!(got.len(), 3, "a note and the two newest lines");
        assert!(
            got[0].1.contains("1 earlier lines were dropped"),
            "{}",
            got[0].1
        );
        assert_eq!((got[1].0, got[2].0), (1, 2));
        assert!(h.take().is_empty());
    }

    #[tokio::test]
    async fn lines_are_cut_at_newlines_at_the_event_size_and_at_the_end() {
        let long = vec![b'a'; LINE_MAX + 10];
        let mut input = b"one\r\ntwo\n\xffbad\n".to_vec();
        input.extend_from_slice(&long);
        input.extend_from_slice(b"\nlast");
        let mut l = Lines::new(&input[..]);
        let mut got = Vec::new();
        while !l.done {
            l.fill().await;
            while let Some(line) = l.line() {
                got.push(line);
            }
        }
        assert_eq!(&got[..3], ["one", "two", "\u{fffd}bad"]);
        assert_eq!(got[3].len(), LINE_MAX);
        assert_eq!(got[4], "a".repeat(10));
        assert_eq!(got[5], "last");
        assert_eq!(got.len(), 6);
    }
}
