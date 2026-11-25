//! What each forced event teaches: what it stands for in production, what to expect when it is forced here, what a well-behaved client
//! does, the settings that decide it (with the defaults AWS documents, read from the vendored models or the doc comments of the code that
//! implements them), and where to see it in this console. Static words, written once; the page reads them from
//! `GET /api/v1/lab/once/guide`.

use super::kinds::{KINDS, REST};
use crate::model::{LabOnceGuide, LabOnceSetting, LabOnceTeach};

fn s(text: &str) -> String {
    text.to_owned()
}

fn set(name: &str, side: &str, default: &str, why: &str) -> LabOnceSetting {
    LabOnceSetting {
        name: s(name),
        side: s(side),
        default: s(default),
        why: s(why),
    }
}

/// The guide: how forced events differ from rules, and one lesson per kind in the order of [`KINDS`].
#[must_use]
pub fn guide() -> LabOnceGuide {
    LabOnceGuide {
        intro: s(
            "A rule bends the calls your app makes. A forced event is the other half of a rehearsal: something production does by itself, on a timer, that your app has to live with. Here you ask for it now. The service carries it out with the function its own timer or request path runs, never with a transition made up for the Lab, and says in words when there is nothing to do.",
        ),
        kinds: KINDS.iter().filter_map(|k| teach(k.id)).collect(),
    }
}

/// The lesson of the kind called `id`.
#[must_use]
pub fn teach(id: &str) -> Option<LabOnceTeach> {
    let title = super::kinds::find(id)?.title;
    let (production, expect, client, settings, see) = match id {
        REST => rest(),
        aws_wire::forced::kind::FIRE_SCHEDULE => schedule(),
        aws_wire::forced::kind::EXPIRE_VISIBILITY => visibility(),
        aws_wire::forced::kind::SWEEP_TTL => ttl(),
        aws_wire::forced::kind::COLD_START => cold(),
        _ => return None,
    };
    Some(LabOnceTeach {
        kind: s(id),
        title: s(title),
        production: s(production),
        expect: s(expect),
        client: s(client),
        settings,
        see: s(see),
    })
}

type Lesson = (
    &'static str,
    &'static str,
    &'static str,
    Vec<LabOnceSetting>,
    &'static str,
);

fn rest() -> Lesson {
    (
        "AWS has no service for you to rest: this one is doze-aws's own. doze-aws keeps a service in memory only while something calls it, and closes it after --idle-secs without a call. It matters because the first call after a quiet spell pays to open the service's files, so a test with a tight timeout can pass when everything is warm and fail when it is not, and because a timer or schedule that comes due still starts the service by itself.",
        "The service closes as it does when idle: it answers what it already holds, closes its files and frees its memory. Its data stays on disk. The next call to it in that region opens it again and is answered as before, but it takes longer than the calls after it. With the option on, the Lab makes that call for you and tells you both times; Live shows them as calls from the console.",
        "Do not depend on the first call being as fast as the rest: give calls a timeout that allows for a slow first call, and retry a call that times out. Do not keep state in a service that you cannot read back after it has been closed.",
        vec![set(
            "--idle-secs",
            "doze-aws",
            "300",
            "How long a service may go without a call before it rests.",
        )],
        "Home shows the service as Resting. The call that opens it again is on Live with a longer time than the ones after it.",
    )
}

fn schedule() -> Lesson {
    (
        "EventBridge Scheduler invokes a schedule's target when its time comes, as the schedule's execution role: a message sent to a queue, a function invoked, a state machine started. Your code is called at a time you did not choose, by a caller that is not your app, and a schedule that is retried can call it more than once.",
        "The schedule's target is invoked now, once, with the schedule's input (its <aws.scheduler.*> placeholders filled in), by the path the timer takes. A recurring schedule keeps its plan, so its next fire still happens when it was due; a one-time schedule's fire is its one fire, and it is deleted afterwards if its ActionAfterCompletion says so. A target that refuses is retried by the schedule's retry policy and then goes to its dead-letter queue, if it has one. A disabled schedule, or one with no fire left, is refused with the words.",
        "Treat an invocation as something that can arrive at any time and more than once: make the work idempotent, read the scheduled time from the input and not from the clock, and fail loudly (an error, not a swallowed exception) so that the retry policy and the dead-letter queue see it.",
        vec![
            set(
                "RetryPolicy.MaximumRetryAttempts",
                "schedule target",
                "185",
                "How many times the schedule tries again after a failed invocation.",
            ),
            set(
                "RetryPolicy.MaximumEventAgeInSeconds",
                "schedule target",
                "86400",
                "How long it keeps trying, in seconds.",
            ),
            set(
                "DeadLetterConfig.Arn",
                "schedule target",
                "none",
                "The queue an invocation that gave up is sent to.",
            ),
        ],
        "The schedule's page. Live shows the target's call, marked as the schedule's.",
    )
}

fn visibility() -> Lesson {
    (
        "A consumer that receives a message and does not delete it before its visibility timeout runs out (it crashed, it was slow, it forgot) finds the message delivered again, to it or to another consumer. SQS delivers at least once, and this is how a message comes back.",
        "Every message of the queue that is in flight comes back at once, with the receive count it had, so the next receive shows ApproximateReceiveCount one higher. The receipt handles it was given still name it, so a slow consumer's delete still works. It is what ChangeMessageVisibility with a timeout of 0 does. A queue with a redrive policy moves a message to its dead-letter queue on the receive after the count reaches maxReceiveCount. Messages waiting out a delivery delay are not touched.",
        "Make processing idempotent, since the same message can arrive twice; delete a message only after the work is done; extend the timeout with ChangeMessageVisibility for work that takes long; and give the queue a dead-letter queue so a message that always fails stops coming back.",
        vec![
            set(
                "VisibilityTimeout",
                "queue",
                "30 seconds",
                "How long a received message stays hidden; 0 seconds to 12 hours.",
            ),
            set(
                "maxReceiveCount",
                "queue's redrive policy",
                "none",
                "How many receives before a message moves to the dead-letter queue.",
            ),
            set(
                "WaitTimeSeconds",
                "receive call",
                "0",
                "How long a receive waits for a message; up to 20 seconds.",
            ),
        ],
        "The queue's page shows the messages available and in flight. Live shows the next receive.",
    )
}

fn ttl() -> Lesson {
    (
        "A DynamoDB item whose time-to-live attribute is in the past has expired, and the table deletes it in its own time: its documentation says typically within two days of expiration, and until then the item still shows up in reads, queries and scans. Each deletion is a record on the table's stream, written by the service and not by a user.",
        "doze-aws deletes an expired item when it is due, within about a second, so a sweep seldom finds anything; AWS's is far slower, and this Lab cannot hold a deletion back. When items have expired and not yet gone, the sweep runs the function the region's timer runs, over every table of the region with time to live, and says how many it removed; a table with a stream gets a REMOVE record for each. When nothing has expired it says so, and when the next item will.",
        "Do not assume an expired item is gone: filter on the expiry in the query (a filter expression comparing the TTL attribute with now) wherever stale data would matter. Read the stream's REMOVE records with a user identity of the DynamoDB service as the system's deletions, not a user's.",
        vec![
            set(
                "TimeToLiveSpecification.AttributeName",
                "table",
                "none",
                "The Number attribute that holds each item's expiry, in epoch seconds.",
            ),
            set(
                "Deletion delay",
                "DynamoDB",
                "typically within two days",
                "How long after its expiry an item may still be returned.",
            ),
        ],
        "The table's page and its items. Live shows the reads that follow.",
    )
}

fn cold() -> Lesson {
    (
        "Lambda starts a process for a function when none is warm, and keeps it for the invocations that follow. The first invocation pays for the start and for your initialisation code, and reports it as Init Duration; a function that has been quiet for a while, or whose code or configuration has changed, starts again.",
        "doze-aws ends the function's warm processes by the call it makes when its code or configuration changes (an invocation in progress is let finish), and changes nothing about the function. The next invocation starts a new process, runs your initialisation again, and its REPORT line in CloudWatch Logs carries an Init Duration. A function with no process running is refused with the words: its next invocation is already a cold one.",
        "Keep initialisation light and do expensive set-up outside the handler so that warm invocations reuse it; keep nothing in the process that you cannot rebuild; and set a timeout that allows for the start as well as the work.",
        vec![set(
            "Timeout",
            "function",
            "3 seconds",
            "How long an invocation may run before Lambda stops it; up to 900 seconds. A slow start counts against it.",
        )],
        "The function's page. Its log group holds the REPORT line with the Init Duration.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_is_taught_once_in_the_order_of_the_cards() {
        let g = guide();
        let taught: Vec<&str> = g.kinds.iter().map(|k| k.kind.as_str()).collect();
        let kinds: Vec<&str> = KINDS.iter().map(|k| k.id).collect();
        assert_eq!(taught, kinds);
        for k in &g.kinds {
            for part in [&k.production, &k.expect, &k.client, &k.see, &k.title] {
                assert!(
                    part.len() > 20 || part == &k.title,
                    "{}: a lesson with a part missing",
                    k.kind
                );
                assert!(!part.contains("  "), "{}: {part}", k.kind);
            }
            assert!(!k.settings.is_empty(), "{}", k.kind);
        }
        assert!(teach("nothing").is_none());
    }

    #[test]
    fn the_words_say_never_asleep_and_use_no_marketing() {
        let g = guide();
        let all = serde_json::to_string(
            &g.kinds
                .iter()
                .map(|k| (&k.production, &k.expect, &k.client))
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default();
        let lower = all.to_lowercase();
        for word in ["asleep", "seamless", "powerful", "blazing"] {
            assert!(!lower.contains(word), "{word}");
        }
    }
}
