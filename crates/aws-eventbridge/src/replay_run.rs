//! A replay running: each step, on the region's timer, reads the next batch of the archive's window and puts it back on the bus. A
//! replay with nothing left to read is `COMPLETED` and moves to the replays table; one asked to stop is `CANCELLED` at its next step,
//! with the events it had not yet put back left unsent; one whose archive was deleted under it is `FAILED`.
//!
//! The pace is this module's, not AWS's (AWS gives none): [`STARTING_MS`] before the first batch, then [`BATCH`] events every
//! [`STEP_MS`]. A replay does not hold the actor for longer than a batch, so calls are answered while it runs.

use aws_wire::service::Caller;

use crate::archives::event_key;
use crate::core::Core;
use crate::event::Event;
use crate::model::{Run, RunState, key};

/// How long a replay is `STARTING` before its first batch.
pub(crate) const STARTING_MS: i64 = 1_000;
/// The time between two batches.
const STEP_MS: i64 = 250;
/// The most events a step puts back.
const BATCH: usize = 100;

/// (memory) AWS's reason for a cancelled replay.
const CANCELLED: &str = "Replay was cancelled.";
/// The reason given when the archive went while the replay read it (memory).
const ARCHIVE_GONE: &str = "The archive of the replay was deleted.";

impl Core {
    /// When the next replay step is due, if a replay is going.
    pub(crate) fn next_replay_step(&self) -> Option<i64> {
        self.runs
            .iter()
            .filter(|(_, r)| r.state.going())
            .map(|(_, r)| r.due_ms)
            .min()
    }

    /// Moves on every replay whose step is due.
    pub(crate) async fn step_replays(&mut self) {
        let now = self.now_ms();
        let due: Vec<String> = self
            .runs
            .iter()
            .filter(|(_, r)| r.state.going() && r.due_ms <= now)
            .map(|(k, _)| k.to_owned())
            .collect();
        for k in due {
            self.step(&k, now).await;
        }
    }

    async fn step(&mut self, k: &str, now: i64) {
        let Some(run) = self.runs.get(k).cloned() else {
            return;
        };
        if run.state == RunState::Cancelling {
            self.stop(k, RunState::Cancelled, CANCELLED, now);
            return;
        }
        if self
            .archives
            .get(&key(&run.replay.account, &[&run.archive]))
            .is_none()
        {
            self.stop(k, RunState::Failed, ARCHIVE_GONE, now);
            return;
        }
        let (read, last) = self.put_back(&run).await;
        let Some(next) = self.runs.get_mut(k) else {
            return;
        };
        next.state = RunState::Running;
        if let Some((cursor, at)) = last {
            next.cursor = Some(cursor);
            next.replay.last_replayed_ms = Some(at);
        }
        next.due_ms = now + STEP_MS;
        if read < BATCH {
            self.complete(k, now);
        }
    }

    /// Puts the next batch back on the bus: how many rows were read, and the key and time of the last event.
    async fn put_back(&mut self, run: &Run) -> (usize, Option<(String, i64)>) {
        let account = &run.replay.account;
        let from = run.cursor.as_ref().map_or_else(
            || event_key(account, &run.archive, run.replay.start_ms, ""),
            // The smallest key after the cursor.
            |c| format!("{c}\u{0}"),
        );
        let to = event_key(account, &run.archive, run.replay.end_ms, "");
        let Ok(rows) = self
            .archived
            .scan(self.db.as_ref(), &from, &to, BATCH, false)
        else {
            // WHY: a failed read is tried again at the next step; the replay is not lost to a passing fault.
            return (BATCH, None);
        };
        let read = rows.len();
        let caller: Caller = run.caller.clone().into();
        let only = (!run.only.is_empty()).then_some(run.only.as_slice());
        let mut last = None;
        for (row_key, bytes) in rows {
            let event = std::str::from_utf8(&bytes).ok().and_then(Event::of_text);
            if let Some(mut event) = event {
                event.replay_name = Some(run.replay.name.clone());
                let at = event.time_ms;
                self.publish(&caller, account, &run.bus, event, only).await;
                last = Some((row_key, at));
            } else {
                last = Some((row_key, run.replay.last_replayed_ms.unwrap_or_default()));
            }
        }
        (read, last)
    }

    /// The replay read its whole window: it moves to the replays table as `COMPLETED`.
    fn complete(&mut self, k: &str, now: i64) {
        if let Some(run) = self.runs.remove(k) {
            let mut replay = run.replay;
            replay.ended_ms = now;
            self.replays.insert(k.to_owned(), replay);
        }
    }

    fn stop(&mut self, k: &str, state: RunState, reason: &str, now: i64) {
        if let Some(run) = self.runs.get_mut(k) {
            run.state = state;
            run.reason = Some(reason.to_owned());
            run.replay.ended_ms = now;
        }
    }
}
