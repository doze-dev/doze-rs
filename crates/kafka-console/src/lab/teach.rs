//! The Lab's teaching: how it works, and for each rehearsal what it stands for in production, what a well-behaved client does,
//! the settings that decide it (with the Java client's defaults, Kafka 4) and where to see it here. Static words, written once:
//! the page, the dialog and the rule's sheet all read them from `GET /api/v1/lab/guide`.

use crate::model::lab_guide::{LabIntro, LabSetting, LabStep, LabTeach};

/// The dialog's tabs, in order.
pub const TABS: [&str; 6] = ["slow", "fail", "drop", "expire", "rebalance", "compact"];

fn s(text: &str) -> String {
    text.to_owned()
}

/// How the Lab works.
#[must_use]
pub fn intro() -> LabIntro {
    LabIntro {
        what: s(
            "Rehearse the failures your app will meet in production (a slow broker, an error, a dropped connection, a \
             rebalance) safely, on your laptop, and watch what your app does about them.",
        ),
        steps: vec![
            LabStep {
                title: s("Choose a rehearsal"),
                text: s(
                    "Start from a recipe, or pick what to bend: which requests, from which app, how often.",
                ),
            },
            LabStep {
                title: s("Start it, then use your app"),
                text: s(
                    "The rule acts on the requests it matches until it ends by itself. Each one it touches is tagged lab.",
                ),
            },
            LabStep {
                title: s("Watch, then stop"),
                text: s(
                    "Follow the tagged requests on Live and the group's Story. Remove one rule, or Stop all.",
                ),
            },
        ],
        limits: vec![
            s("Nothing is saved: rules last only while this broker runs, and a restart clears them."),
            s(
                "Only this broker, and only the requests a rule matches. Slow, Fail and Drop change answers, never your records.",
            ),
            s(
                "Every rule ends by itself. Stop all, at the top of this page, ends every one at once.",
            ),
        ],
    }
}

fn set(name: &str, side: &str, default: &str, why: &str) -> LabSetting {
    LabSetting {
        name: s(name),
        side: s(side),
        default: s(default),
        why: s(why),
    }
}

fn slow() -> LabTeach {
    LabTeach {
        tab: s("slow"),
        title: s("Slow requests"),
        lede: s("Delay the answer to the requests you choose. Your apps see slow, not broken."),
        simulates: s(
            "A broker or network under load: a disk that stalls, a long pause on the broker, a link across regions.",
        ),
        expect: s(
            "The matched requests take the delay longer. A healthy app slows down and loses nothing; past its \
                   request.timeout.ms it retries instead.",
        ),
        client: vec![
            s(
                "Keep sending while acks are late: the producer buffers records and only blocks send() when the buffer is full.",
            ),
            s(
                "Retry a request that outlives request.timeout.ms; with idempotence a retry is never written twice.",
            ),
            s(
                "Consumers: keep processing per poll well under max.poll.interval.ms, or the group drops the member.",
            ),
        ],
        settings: vec![
            set(
                "request.timeout.ms",
                "client",
                "30000",
                "How long to wait for one answer before retrying it.",
            ),
            set(
                "delivery.timeout.ms",
                "producer",
                "120000",
                "The whole budget for a record, retries included; then send() fails.",
            ),
            set(
                "max.block.ms",
                "producer",
                "60000",
                "How long send() blocks on a full buffer or missing metadata.",
            ),
            set(
                "max.poll.interval.ms",
                "consumer",
                "300000",
                "Longest gap between two polls before the member is removed.",
            ),
        ],
        watch: s(
            "On Live, the requests it touched carry the lab tag and their Took grows by the delay. A group's Lag tab \
                  shows lag climb while it runs and drain after.",
        ),
    }
}

fn fail() -> LabTeach {
    LabTeach {
        tab: s("fail"),
        title: s("Fail some requests"),
        lede: s(
            "Answer 1 in N of the requests you choose with an error Kafka has, without asking the broker.",
        ),
        simulates: s(
            "A broker answering with an error: NOT_LEADER_OR_FOLLOWER while leadership moves in a rolling restart, \
                      REQUEST_TIMED_OUT under overload, UNKNOWN_TOPIC_OR_PARTITION on stale metadata.",
        ),
        expect: s(
            "The matched requests fail at once with the code you chose; the broker never sees them, so nothing is \
                   written for them. A well-behaved producer retries and every record arrives once.",
        ),
        client: vec![
            s(
                "Retry a retriable error after a backoff that grows, until delivery.timeout.ms is spent.",
            ),
            s("On NOT_LEADER_OR_FOLLOWER, refresh metadata and send to the leader it names."),
            s(
                "Keep enable.idempotence on (the default since Kafka 3.0): a retried batch is written once, in order.",
            ),
            s("Only surface the error to your code when the retries are spent."),
        ],
        settings: vec![
            set(
                "retries",
                "producer",
                "2147483647",
                "How many times to retry; delivery.timeout.ms ends it first.",
            ),
            set(
                "retry.backoff.ms",
                "client",
                "100",
                "The first wait before a retry.",
            ),
            set(
                "retry.backoff.max.ms",
                "client",
                "1000",
                "The longest wait, as the backoff grows.",
            ),
            set(
                "enable.idempotence",
                "producer",
                "true",
                "Retries cannot duplicate or reorder records.",
            ),
            set(
                "max.in.flight.requests.per.connection",
                "producer",
                "5",
                "At most 5 keeps order with idempotence.",
            ),
        ],
        watch: s(
            "On Live, filtered to this rule: each failed request is tagged lab, and the client's retry follows it. On \
                  the topic's Messages, check that nothing arrived twice.",
        ),
    }
}

fn drop() -> LabTeach {
    LabTeach {
        tab: s("drop"),
        title: s("Drop a connection"),
        lede: s("Close one app's connections without an answer, once or for a few minutes."),
        simulates: s(
            "A network partition, a broker crash or restart, a load balancer or NAT cutting a connection.",
        ),
        expect: s(
            "The app's connections close with no answer. It should reconnect within a second or two and carry on \
                   from where it was.",
        ),
        client: vec![
            s("Reconnect by itself, waiting a little longer after each failed try."),
            s("Retry the requests that were in flight; with idempotence, without duplicates."),
            s("Consumers: back within session.timeout.ms, the member keeps its partitions."),
        ],
        settings: vec![
            set(
                "reconnect.backoff.ms",
                "client",
                "50",
                "The first wait before reconnecting.",
            ),
            set(
                "reconnect.backoff.max.ms",
                "client",
                "1000",
                "The longest wait between tries.",
            ),
            set(
                "socket.connection.setup.timeout.ms",
                "client",
                "10000",
                "How long one connection attempt may take.",
            ),
            set(
                "session.timeout.ms",
                "consumer",
                "45000",
                "How long a member may be silent before it is removed.",
            ),
        ],
        watch: s(
            "On Live, the dropped requests show as Closed with the lab tag; the app's next requests arrive on a new \
                  connection.",
        ),
    }
}

fn expire() -> LabTeach {
    LabTeach {
        tab: s("expire"),
        title: s("Expire a member's session"),
        lede: s("Remove one member of a group, as a missed heartbeat would."),
        simulates: s(
            "A consumer that stopped heartbeating: a long garbage-collection pause, a frozen container, a laptop \
                      lid closed. The coordinator removes it after session.timeout.ms.",
        ),
        expect: s(
            "The member is removed at once and the group rebalances; the others take its partitions until it \
                   notices and joins again.",
        ),
        client: vec![
            s(
                "Notice it was removed (its next heartbeat or commit is refused with UNKNOWN_MEMBER_ID) and join again.",
            ),
            s(
                "Not commit for partitions it lost: the next owner reads from the last committed offset.",
            ),
            s("Process idempotently: a record handled but not committed is read again."),
        ],
        settings: vec![
            set(
                "session.timeout.ms",
                "consumer",
                "45000",
                "Silence allowed before the coordinator removes a member.",
            ),
            set(
                "heartbeat.interval.ms",
                "consumer",
                "3000",
                "How often a member says it is alive.",
            ),
            set(
                "enable.auto.commit",
                "consumer",
                "true",
                "Commits on a timer; a crash between two commits rereads records.",
            ),
            set(
                "auto.commit.interval.ms",
                "consumer",
                "5000",
                "The timer of the automatic commit.",
            ),
        ],
        watch: s(
            "The group's Story tells it in order: the member removed, the rebalance, the new assignment. Live shows \
                  its next heartbeat refused, then a JoinGroup.",
        ),
    }
}

fn rebalance() -> LabTeach {
    LabTeach {
        tab: s("rebalance"),
        title: s("Force a rebalance"),
        lede: s("Start a rebalance of one group now, without waiting for a reason."),
        simulates: s(
            "A deploy: a consumer added or removed, a rolling restart, a changed subscription. Every member gives \
                      up or confirms its partitions.",
        ),
        expect: s(
            "The group prepares a rebalance, every member joins again and partitions are assigned anew (often the \
                   same ones). Processing pauses briefly.",
        ),
        client: vec![
            s("Commit what it processed before its partitions are revoked."),
            s(
                "Pause and resume cleanly; with the cooperative protocol only the partitions that move pause.",
            ),
            s(
                "Not process a record twice: what was not committed is read again by the next owner.",
            ),
        ],
        settings: vec![
            set(
                "partition.assignment.strategy",
                "consumer",
                "Range, CooperativeSticky",
                "How partitions are shared out.",
            ),
            set(
                "group.protocol",
                "consumer",
                "classic",
                "consumer is the new protocol: rebalances move only what changes.",
            ),
            set(
                "group.instance.id",
                "consumer",
                "none",
                "Static membership: a restart within the session causes no rebalance.",
            ),
            set(
                "max.poll.interval.ms",
                "consumer",
                "300000",
                "A member that polls late misses the rebalance and is removed.",
            ),
        ],
        watch: s(
            "The group's Story shows the generation go up and who got which partitions; its Lag tab shows the pause \
                  in commits.",
        ),
    }
}

fn compact() -> LabTeach {
    LabTeach {
        tab: s("compact"),
        title: s("Run the cleaner now"),
        lede: s("Clean one compacted partition now instead of at the cleaner's next look."),
        simulates: s(
            "What the log cleaner does on its own schedule: only the newest record of each key stays, and a key \
                      whose newest value is empty (a tombstone) goes after delete.retention.ms.",
        ),
        expect: s(
            "Older records of each key leave the cleaned segments; the active segment is never cleaned. Offsets keep \
                   their numbers, with gaps.",
        ),
        client: vec![
            s(
                "Read a compacted topic as a table: from the start, keeping the latest value of each key.",
            ),
            s("Not rely on older values of a key, or on every offset being there."),
            s("Delete a key by writing a tombstone: the key with an empty value."),
        ],
        settings: vec![
            set(
                "cleanup.policy",
                "topic",
                "delete",
                "compact keeps the newest record of each key.",
            ),
            set(
                "min.cleanable.dirty.ratio",
                "topic",
                "0.5",
                "How much of the log must be uncleaned before a round.",
            ),
            set(
                "delete.retention.ms",
                "topic",
                "86400000",
                "How long a tombstone stays for readers to see it.",
            ),
            set(
                "min.compaction.lag.ms",
                "topic",
                "0",
                "How old a record must be before it can be cleaned.",
            ),
        ],
        watch: s(
            "The topic's Log tab shows the segments the round rewrote; its Messages show the keys that stayed.",
        ),
    }
}

/// Every rehearsal, taught, in the order of [`TABS`].
#[must_use]
pub fn rehearsals() -> Vec<LabTeach> {
    vec![slow(), fail(), drop(), expire(), rebalance(), compact()]
}

#[cfg(test)]
mod tests;
