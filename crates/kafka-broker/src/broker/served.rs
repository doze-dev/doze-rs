//! A client's request, held to the client's quotas: recorded as Kafka records it, answered with the throttle time, and the
//! connection told how long to stop reading from the client (KIP-219: the answer goes at once, and the broker mutes the
//! channel for the throttle time rather than holding the answer back).

use std::future::Future;
use std::pin::pin;
use std::time::{Duration, Instant};

use kafka_storage::fs::Fs;
use kafka_wire::api::{Request, Response};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::{ApiKey, FetchResponse};

use super::Broker;
use crate::quotas::{Client, Mutations, QuotaKind};
use crate::{Caller, Listener, Outcome, clock};

/// What became of a client's request: the outcome to act on, and how long the connection then reads nothing more from the
/// client (none, unless a quota was exceeded).
#[derive(Debug)]
pub struct Served {
    pub outcome: Outcome,
    pub mute: Option<Duration>,
}

/// The first version of each API that makes the controller mutation quota strict (THROTTLING_QUOTA_EXCEEDED); below it
/// the mutation is made and the client throttled (`ControllerApis`: `newQuotaFor(..., strictSinceVersion)`).
fn strict_since(key: ApiKey) -> Option<i16> {
    match key {
        ApiKey::CreateTopics => Some(6),
        ApiKey::DeleteTopics => Some(5),
        ApiKey::CreatePartitions => Some(3),
        _ => None,
    }
}

impl<F: Fs> Broker<F> {
    /// Handles a client's request as [`Broker::handle`] does, held to its quotas. `size` is the request's size on the wire
    /// (header and body, as Kafka's `Request.sizeInBytes`), what a Produce counts against `producer_byte_rate`.
    ///
    /// With no quota set this is `handle` and one atomic load. With some set, as `KafkaApis` does for each API:
    ///
    /// - every request's time on a request thread counts against `request_percentage` (a Produce with `acks=0` excepted);
    ///   here the time is the time the broker spent working on it, the waits of a long-poll Fetch or of a lock left out;
    /// - a Produce counts its size against `producer_byte_rate`;
    /// - a Fetch asks for no more than the client may be sent in one go, counts its answer's size against
    ///   `consumer_byte_rate`, and if it is throttled for either quota is answered empty, its bytes taken back;
    /// - CreateTopics, CreatePartitions and DeleteTopics count their partitions against `controller_mutation_rate`.
    ///
    /// The throttle time is the longest of the quotas exceeded, and the answer carries it; a request from a follower (a
    /// Fetch with a replica id) is exempt from all of it.
    pub async fn serve(
        &self,
        listener: &Listener,
        caller: &Caller,
        head: &RequestHead,
        mut request: Request,
        size: usize,
    ) -> Served {
        let follower = matches!(&request, Request::Fetch(r) if r.replica_id.0 >= 0);
        if !self.quotas.enabled() || follower {
            return Served {
                outcome: self.handle(listener, caller, head, request).await,
                mute: None,
            };
        }
        let who = Client {
            user: caller.principal().name(),
            id: head.client_id.as_deref().unwrap_or(""),
        };
        let unacknowledged = matches!(&request, Request::Produce(r) if r.acks == 0);
        self.cap_fetch(&mut request, who);
        let mut mutations = match strict_since(head.key) {
            Some(v) => self.quotas.mutations(who, head.version >= v),
            None => Mutations::unbounded(),
        };
        let (mut outcome, busy) =
            busy_time(self.dispatch(listener, caller, head, request, &mut mutations)).await;
        let now = clock::now().get();
        let request_ms = if unacknowledged {
            0
        } else {
            self.quotas
                .record(QuotaKind::Request, who, percent_of_a_second(busy), now)
        };
        let other = match head.key {
            ApiKey::Produce => {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a request's size, as Kafka records it"
                )]
                let bytes = size as f64;
                let ms = self.quotas.record(QuotaKind::Produce, who, bytes, now);
                (QuotaKind::Produce, ms)
            }
            ApiKey::Fetch => (
                QuotaKind::Fetch,
                self.fetch_quota(&mut outcome, head, who, request_ms, now),
            ),
            ApiKey::CreateTopics | ApiKey::DeleteTopics | ApiKey::CreatePartitions => {
                (QuotaKind::Mutation, mutations.throttle_time(now))
            }
            _ => (QuotaKind::Request, 0),
        };
        self.throttled(outcome, who, request_ms, other, now)
    }

    /// A consumer's Fetch asks for no more than it may be sent in one go (`KafkaApis.handleFetchRequest`: `fetchMaxBytes` is cut
    /// to `maxValueInQuotaWindow`, and `fetchMinBytes` to that).
    fn cap_fetch(&self, request: &mut Request, who: Client<'_>) {
        if let Request::Fetch(r) = request
            && let Some(most) = self.quotas.most_in_window(QuotaKind::Fetch, who)
        {
            // Java's `(int)` of the double: saturating, as `as` is.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "saturates, as Java's cast does"
            )]
            let most = most as i32;
            r.max_bytes = r.max_bytes.min(most);
            r.min_bytes = r.min_bytes.min(r.max_bytes);
        }
    }

    /// The outcome with the longer of the request quota's throttle and the other quota's (`other`), noted for the console and
    /// written into the answer, and the mute it comes to. On a tie the request quota is the one blamed, as in Kafka.
    fn throttled(
        &self,
        mut outcome: Outcome,
        who: Client<'_>,
        request_ms: i32,
        (other, other_ms): (QuotaKind, i32),
        now: i64,
    ) -> Served {
        let throttle = request_ms.max(other_ms);
        if throttle == 0 {
            return Served {
                outcome,
                mute: None,
            };
        }
        let blamed = if other_ms > request_ms {
            other
        } else {
            QuotaKind::Request
        };
        self.quotas.throttled(blamed, who, throttle, now);
        if let Outcome::Respond(response) = &mut outcome {
            response.set_throttle_time_ms(throttle);
        }
        Served {
            outcome,
            mute: Some(Duration::from_millis(u64::try_from(throttle).unwrap_or(0))),
        }
    }

    /// A Fetch's answer against `consumer_byte_rate`: its size recorded, and if the client is throttled for this or for the
    /// request quota, the size taken back and the answer emptied (`KafkaApis.handleFetchRequest`,
    /// `FetchContext.getThrottledResponse`: no error, no session, no partitions).
    fn fetch_quota(
        &self,
        outcome: &mut Outcome,
        head: &RequestHead,
        who: Client<'_>,
        request_ms: i32,
        now: i64,
    ) -> i32 {
        let Outcome::Respond(response @ Response::Fetch(_)) = outcome else {
            return 0;
        };
        // `FetchResponse.sizeOf`: the body, and four bytes for the header.
        #[expect(
            clippy::cast_precision_loss,
            reason = "a response's size, as Kafka records it"
        )]
        let bytes = response
            .compute_size(head.version)
            .map_or(0.0, |n| (n + 4) as f64);
        let ms = self.quotas.record(QuotaKind::Fetch, who, bytes, now);
        if ms.max(request_ms) > 0 {
            self.quotas.unrecord(QuotaKind::Fetch, who, bytes, now);
            *response = Response::Fetch(FetchResponse::default());
        }
        ms
    }
}

/// A second of one thread's time is 100 percent (`ClientRequestQuotaManager.nanosToPercentage`).
fn percent_of_a_second(busy: Duration) -> f64 {
    busy.as_secs_f64() * 100.0
}

/// Runs `work` to its end and adds up the time spent inside it: the time a thread was working on the request, and not the
/// time it was waiting (for data to arrive at a long-poll Fetch, for a lock, for the disk on the blocking pool). The nearest
/// thing to Kafka's request-thread time an async broker has.
async fn busy_time<T>(work: impl Future<Output = T>) -> (T, Duration) {
    let mut work = pin!(work);
    let mut busy = Duration::ZERO;
    let out = std::future::poll_fn(|cx| {
        let start = Instant::now();
        let polled = work.as_mut().poll(cx);
        busy += start.elapsed();
        polled
    })
    .await;
    (out, busy)
}
