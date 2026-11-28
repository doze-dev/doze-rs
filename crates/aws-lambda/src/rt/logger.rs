//! Writing a function's log to CloudWatch Logs. An invocation's caller does not wait for its log: the events are queued, and one task (started when
//! there is something to write, ended when the queue is empty) writes them in order, one `PutLogEvents` per stream for everything
//! queued for it, as Lambda's own log arrives a moment after the response. The queue holds at most [`QUEUED_EVENTS`]; past that an
//! invocation waits for room, so a function that prints faster than the log is written slows down instead of growing the heap.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use aws_peers::Peers;
use aws_wire::service::Caller;
use doze_runtime::Tasks;
use serde_json::json;
use tokio::sync::Notify;

/// The most events waiting to be written.
pub(crate) const QUEUED_EVENTS: usize = 20_000;
/// The most events of one `PutLogEvents` (CloudWatch Logs' quota is 10,000 events and 1 MiB).
const PUT_EVENTS: usize = 10_000;
const PUT_BYTES: usize = 1024 * 1024;

/// Where a function's output goes.
#[derive(Clone, Debug, Default)]
pub(crate) struct Logger {
    state: Arc<State>,
}

#[derive(Debug, Default)]
struct State {
    /// Groups and streams already created, so they are made once.
    made: Mutex<HashSet<String>>,
    queue: Mutex<Queue>,
    /// Told when the writer has taken from the queue.
    room: Notify,
}

#[derive(Debug, Default)]
struct Queue {
    jobs: VecDeque<Job>,
    events: usize,
    writing: bool,
}

/// Events for one stream.
#[derive(Debug)]
struct Job {
    caller: Caller,
    group: String,
    stream: String,
    events: Vec<(i64, String)>,
}

impl Logger {
    /// Queues `events` for the function's log group and stream; they are written by the logger's task, in the order queued. Waits
    /// only when the queue is full.
    pub(crate) async fn send(
        &self,
        tasks: &Tasks,
        peers: Option<&Arc<Peers>>,
        caller: &Caller,
        (group, stream): (&str, &str),
        events: Vec<(i64, String)>,
    ) {
        let Some(peers) = peers else { return };
        if events.is_empty() {
            return;
        }
        loop {
            let room = self.state.room.notified();
            tokio::pin!(room);
            room.as_mut().enable();
            {
                let mut q = self
                    .state
                    .queue
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if q.events == 0 || q.events + events.len() <= QUEUED_EVENTS {
                    q.events += events.len();
                    q.jobs.push_back(Job {
                        caller: caller.clone(),
                        group: group.to_owned(),
                        stream: stream.to_owned(),
                        events,
                    });
                    if !q.writing {
                        q.writing = true;
                        tasks.spawn(self.clone().drain(Arc::clone(peers)));
                    }
                    return;
                }
            }
            room.await;
        }
    }

    /// Whether nothing waits to be written (for tests).
    #[cfg(test)]
    pub(crate) fn idle(&self) -> bool {
        !self
            .state
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .writing
    }

    /// Writes what is queued until the queue is empty: each stream's events in one call (or as few as the quota allows).
    async fn drain(self, peers: Arc<Peers>) {
        loop {
            let jobs: Vec<Job> = {
                let mut q = self
                    .state
                    .queue
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if q.jobs.is_empty() {
                    q.writing = false;
                    return;
                }
                q.events = 0;
                q.jobs.drain(..).collect()
            };
            self.state.room.notify_waiters();
            for j in by_stream(jobs) {
                for put in batches(j.events) {
                    self.write(&peers, &j.caller, &j.group, &j.stream, put)
                        .await;
                }
            }
        }
    }

    /// Writes `events` to the function's log group and stream. A failure is logged and does not fail the invocation.
    async fn write(
        &self,
        peers: &Peers,
        caller: &Caller,
        group: &str,
        stream: &str,
        events: Vec<(i64, String)>,
    ) {
        let acting = Caller {
            service: Some("lambda".to_owned()),
            ..caller.clone()
        };
        let key = format!("{}/{group}/{stream}", caller.region);
        let known = self
            .state
            .made
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&key);
        if !known {
            for (op, body) in [
                ("CreateLogGroup", json!({"logGroupName": group})),
                (
                    "CreateLogStream",
                    json!({"logGroupName": group, "logStreamName": stream}),
                ),
            ] {
                if let Err(e) = peers.call("lambda", &acting, "logs", op, &body).await
                    && e.code != "ResourceAlreadyExistsException"
                {
                    tracing::warn!("lambda could not write its log: {op}: {}", e.message);
                    return;
                }
            }
            self.state
                .made
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(key.clone());
        }
        let rows: Vec<_> = events
            .iter()
            .map(|(t, m)| json!({"timestamp": t, "message": m}))
            .collect();
        let put = json!({"logGroupName": group, "logStreamName": stream, "logEvents": rows});
        if let Err(e) = peers
            .call("lambda", &acting, "logs", "PutLogEvents", &put)
            .await
        {
            // The group or stream was deleted under a warm process: the next write makes them again.
            self.state
                .made
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&key);
            tracing::warn!("lambda could not write its log: {}", e.message);
        }
    }
}

/// Jobs joined per stream (region, account, group, stream), each stream's events in the order queued, the streams in the order first
/// seen.
fn by_stream(jobs: Vec<Job>) -> Vec<Job> {
    let mut out: Vec<Job> = Vec::new();
    for j in jobs {
        let same = |o: &&mut Job| {
            o.caller.region == j.caller.region
                && o.caller.account_id == j.caller.account_id
                && o.group == j.group
                && o.stream == j.stream
        };
        match out.iter_mut().find(same) {
            Some(o) => o.events.extend(j.events),
            None => out.push(j),
        }
    }
    out
}

/// `events` cut into `PutLogEvents` calls within CloudWatch Logs' quota.
fn batches(events: Vec<(i64, String)>) -> Vec<Vec<(i64, String)>> {
    let mut out = Vec::new();
    let (mut cur, mut bytes) = (Vec::new(), 0);
    for e in events {
        let size = e.1.len() + 26;
        if !cur.is_empty() && (cur.len() >= PUT_EVENTS || bytes + size > PUT_BYTES) {
            out.push(std::mem::take(&mut cur));
            bytes = 0;
        }
        bytes += size;
        cur.push(e);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_events_are_joined_per_stream_and_cut_to_the_quota() {
        let caller = |region: &str| Caller {
            access_key_id: String::new(),
            account_id: "1".to_owned(),
            region: region.to_owned(),
            endpoint: String::new(),
            service: None,
            session_token: None,
        };
        let job = |region: &str, stream: &str, n: i64| Job {
            caller: caller(region),
            group: "g".to_owned(),
            stream: stream.to_owned(),
            events: (0..n).map(|i| (i, format!("{stream}{i}"))).collect(),
        };
        let joined = by_stream(vec![
            job("r", "a", 2),
            job("r", "b", 1),
            job("r", "a", 1),
            job("q", "a", 1),
        ]);
        let shape: Vec<(&str, &str, usize)> = joined
            .iter()
            .map(|j| (j.caller.region.as_str(), j.stream.as_str(), j.events.len()))
            .collect();
        assert_eq!(shape, [("r", "a", 3), ("r", "b", 1), ("q", "a", 1)]);
        assert_eq!(
            joined[0].events[2].1, "a0",
            "a stream's events stay in the order queued"
        );
        let many: Vec<(i64, String)> = (0..25_000).map(|i| (i, "x".to_owned())).collect();
        let sizes: Vec<usize> = batches(many).iter().map(Vec::len).collect();
        assert_eq!(sizes, [10_000, 10_000, 5_000]);
        let big: Vec<(i64, String)> = (0..5).map(|i| (i, "y".repeat(300_000))).collect();
        assert_eq!(batches(big).len(), 2, "1 MiB a call");
    }
}
