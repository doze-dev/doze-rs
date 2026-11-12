//! A function's log, written while it runs: what it prints reaches CloudWatch Logs every 200 ms, starting with the invocation's
//! `START`, so `aws logs tail --follow` shows a long invocation as it goes (as Lambda's own log does) and not only at its end. The rest
//! (the last lines, `END`, `REPORT`) is written when the invocation ends, by [`Live::unsent`].

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use aws_wire::service::Caller;

use crate::Inner;
use crate::rt::logs::{line_event, start_event};
use crate::rt::worker::{Line, Worker};

/// How often what a running function printed is written.
const EVERY: Duration = Duration::from_millis(200);

/// One invocation's log as it is written.
pub(crate) struct Live<'a> {
    pub(crate) tap: Arc<Worker>,
    pub(crate) group: &'a str,
    pub(crate) caller: &'a Caller,
    pub(crate) start_ms: i64,
    pub(crate) id: &'a str,
    pub(crate) version: &'a str,
    pub(crate) json: bool,
    pub(crate) name: &'a str,
    /// The lines already written, in order.
    sent: Vec<Line>,
    /// Whether `START` has been written.
    started: bool,
}

impl<'a> Live<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        tap: Arc<Worker>,
        group: &'a str,
        caller: &'a Caller,
        start_ms: i64,
        id: &'a str,
        version: &'a str,
        json: bool,
        name: &'a str,
    ) -> Self {
        Self {
            tap,
            group,
            caller,
            start_ms,
            id,
            version,
            json,
            name,
            sent: Vec::new(),
            started: false,
        }
    }

    /// Every line of the invocation: those written while it ran, then `rest`.
    pub(crate) fn all_lines(&self, rest: &[Line]) -> Vec<Line> {
        self.sent.iter().chain(rest).cloned().collect()
    }

    /// Of the invocation's whole log (`START`, the lines, `END`, `REPORT`), what has not been written yet.
    pub(crate) fn unsent(&self, rows: Vec<(i64, String)>) -> Vec<(i64, String)> {
        let skip = if self.started { 1 + self.sent.len() } else { 0 };
        rows.into_iter().skip(skip).collect()
    }
}

impl Inner {
    /// Waits for `invocation`, writing what the function prints as it prints it.
    pub(crate) async fn with_live_log<F: Future>(
        &self,
        live: &mut Live<'_>,
        invocation: F,
    ) -> F::Output {
        tokio::pin!(invocation);
        let mut tick = tokio::time::interval(EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tick.tick().await;
        loop {
            tokio::select! {
                done = &mut invocation => return done,
                _ = tick.tick() => self.write_live(live).await,
            }
        }
    }

    async fn write_live(&self, live: &mut Live<'_>) {
        let lines = live.tap.drain();
        if lines.is_empty() {
            return;
        }
        let mut rows = Vec::with_capacity(lines.len() + 1);
        if !live.started {
            rows.push(start_event(live.start_ms, live.id, live.version, live.json));
        }
        rows.extend(lines.iter().map(|l| line_event(l, live.json)));
        self.echo(live.name, &lines);
        let stream = live.tap.spec.log_stream.clone();
        self.logger
            .write(
                self.peers.as_deref(),
                live.caller,
                live.group,
                &stream,
                rows,
            )
            .await;
        live.started = true;
        live.sent.extend(lines);
    }
}
