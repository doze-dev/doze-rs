//! A subscription that outlives its connection: dial, subscribe, hand every frame on; when the daemon goes away, back
//! off (500 ms, doubling to 5 s) and do it again. On its own thread, as the app's per-stack client is
//! (docs/port/app/PLAN.md §4.3). It resumes the journal after the last event it delivered, so nothing is lost across
//! a daemon restart, and it can be told to follow other topics without dropping the connection.

use super::Client;
use super::stream::{Stream, StreamEvent};
use super::wait::block_on;
use crate::{Frame, Subscription, Topic};
use async_channel::{Receiver, RecvError, Sender};
use std::future::{Future, poll_fn};
use std::pin::pin;
use std::task::Poll;
use std::thread;
use std::time::{Duration, Instant};

/// How long to wait between attempts: `first` after the first failure, doubling up to `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    pub first: Duration,
    pub max: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff {
            first: Duration::from_millis(500),
            max: Duration::from_secs(5),
        }
    }
}

impl Backoff {
    /// The wait after `failures` failures in a row (none after none).
    pub fn delay(&self, failures: u32) -> Duration {
        let Some(doublings) = failures.checked_sub(1) else {
            return Duration::ZERO;
        };
        let factor = 1u32.checked_shl(doublings.min(31)).unwrap_or(u32::MAX);
        self.first.saturating_mul(factor).min(self.max)
    }
}

/// What a follower reports.
#[derive(Debug, Clone, PartialEq)]
pub enum FollowEvent {
    /// Dialling: attempt 1 is the first since it was last live.
    Connecting { attempt: u32 },
    /// Connected and subscribed; frames follow. An instances subscription is sent a snapshot early on, so state can be rebuilt.
    Live,
    /// A topic frame.
    Frame(Box<Frame>),
    /// Not connected, and why; the next attempt is in `retry_in`.
    Down { error: String, retry_in: Duration },
}

#[derive(Debug)]
enum Command {
    Resubscribe(Subscription),
    Stop,
}

/// A running follower. Dropping it stops it.
#[derive(Debug)]
pub struct Follow {
    events: Receiver<FollowEvent>,
    control: Sender<Command>,
}

impl Follow {
    pub(crate) fn start(client: Client, sub: Subscription, backoff: Backoff) -> Follow {
        let (out, events) = async_channel::unbounded();
        let (control, commands) = async_channel::unbounded();
        let mut f = Follower {
            client,
            sub,
            backoff,
            out,
            commands,
            failures: 0,
            attempts: 0,
            last_seq: None,
        };
        // A follower that cannot start a thread reports itself down for good: `events` closes at once.
        let _ = thread::Builder::new()
            .name("doze-control-follow".into())
            .spawn(move || f.run());
        Follow { events, control }
    }

    /// The events, for a blocking or an async consumer.
    pub fn events(&self) -> &Receiver<FollowEvent> {
        &self.events
    }

    /// Follows another subscription from now on, on the same connection (a watch count crossed zero).
    pub fn resubscribe(&self, sub: Subscription) {
        let _ = self.control.try_send(Command::Resubscribe(sub));
    }

    /// Stops following and hangs up.
    pub fn stop(&self) {
        let _ = self.control.try_send(Command::Stop);
    }
}

impl Drop for Follow {
    fn drop(&mut self) {
        self.stop();
    }
}

enum Next {
    Stream(StreamEvent),
    Command(Result<Command, RecvError>),
}

/// What ended a session.
enum Flow {
    Stop,
    Lost(String),
    Resubscribe(Subscription),
}

struct Follower {
    client: Client,
    sub: Subscription,
    backoff: Backoff,
    out: Sender<FollowEvent>,
    commands: Receiver<Command>,
    failures: u32,
    /// Dials since it was last live.
    attempts: u32,
    last_seq: Option<u64>,
}

impl Follower {
    fn run(&mut self) {
        loop {
            let Some(error) = self.session() else { return };
            self.failures = self.failures.saturating_add(1);
            let retry_in = self.backoff.delay(self.failures);
            if self
                .out
                .send_blocking(FollowEvent::Down { error, retry_in })
                .is_err()
                || !self.wait(retry_in)
            {
                return;
            }
        }
    }

    /// One connection's life. `None` when the follower is to stop; otherwise why the connection ended.
    fn session(&mut self) -> Option<String> {
        self.attempts = self.attempts.saturating_add(1);
        let attempt = self.attempts;
        self.out
            .send_blocking(FollowEvent::Connecting { attempt })
            .ok()?;
        let conn = match self.client.dial() {
            Ok(c) => c,
            Err(e) => return Some(e.to_string()),
        };
        let mut live = false;
        loop {
            let stream = match conn.subscribe(&self.resumed()) {
                Ok(s) => s,
                Err(e) => return Some(e.to_string()),
            };
            if !live {
                live = true;
                (self.failures, self.attempts) = (0, 0);
                self.out.send_blocking(FollowEvent::Live).ok()?;
            }
            match self.pump(&stream) {
                Flow::Stop => return None,
                Flow::Lost(why) => return Some(why),
                Flow::Resubscribe(sub) => self.sub = sub,
            }
        }
    }

    /// The subscription, resuming the journal after the last event delivered.
    fn resumed(&self) -> Subscription {
        let mut sub = self.sub.clone();
        if let Some(seq) = self.last_seq.filter(|_| sub.wants(Topic::Journal)) {
            sub.since = seq;
        }
        sub
    }

    /// Hands frames on until the stream ends or a command comes.
    fn pump(&mut self, stream: &Stream) -> Flow {
        loop {
            let next = {
                let mut frame = pin!(stream.frames().recv());
                let mut command = pin!(self.commands.recv());
                block_on(
                    poll_fn(|cx| {
                        if let Poll::Ready(r) = frame.as_mut().poll(cx) {
                            return Poll::Ready(Next::Stream(stream.event(r)));
                        }
                        command.as_mut().poll(cx).map(Next::Command)
                    }),
                    None,
                )
            };
            match next {
                Some(Next::Stream(event)) => match event {
                    StreamEvent::Frame(f) => {
                        if let Some(e) = f.event.as_ref().filter(|e| e.seq > 0) {
                            self.last_seq = Some(e.seq);
                        }
                        if self.out.send_blocking(FollowEvent::Frame(f)).is_err() {
                            return Flow::Stop;
                        }
                    }
                    StreamEvent::End(e) => {
                        return Flow::Lost(
                            e.unwrap_or_else(|| "the daemon ended the stream".into()),
                        );
                    }
                    StreamEvent::Lost(why) => return Flow::Lost(why),
                },
                Some(Next::Command(Ok(Command::Resubscribe(sub)))) => {
                    return Flow::Resubscribe(sub);
                }
                Some(Next::Command(_)) | None => return Flow::Stop,
            }
        }
    }

    /// Sleeps `d`, taking in new subscriptions meanwhile. `false` if told to stop.
    fn wait(&mut self, d: Duration) -> bool {
        let deadline = Instant::now() + d;
        loop {
            match block_on(self.commands.recv(), Some(deadline)) {
                None => return true,
                Some(Ok(Command::Resubscribe(sub))) => self.sub = sub,
                Some(Ok(Command::Stop) | Err(_)) => return false,
            }
        }
    }
}
