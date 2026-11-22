//! What a call did, in a few numbers: the result summary a record keeps beside its status (`3 messages`, `sent 53 B`, `6 keys`).
//!
//! The record keeps no request or answer body, so a summary is read from the input and the answer while they are in hand (the
//! gateway does it as it answers; nothing is parsed, the decoded tree is walked) and held as a [`Summary`]: a kind, a count and
//! a second count, eight bytes. Its words are made only when someone asks ([`Summary::text`]). Which operations have one is the
//! table in `summary/table.rs`: an operation not in it has none, and nothing is guessed for it.
//!
//! A summary says only what the input or the answer says. What the answer cannot say is not said: an SNS `Publish` answers with
//! a message id, so "delivered to 3" is not read from it (the console's record counts the deliveries it saw the bus make for the
//! request, which is a different source and is filled in there).

mod fields;
mod table;
#[cfg(test)]
mod tests;

pub use fields::{Fields, TreeFields};
pub use table::{MOST_DETAIL, from_input, from_output};

/// What kind of result a summary is. The words of each are in [`Summary::text`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    /// No summary: the operation has none, or the call did not succeed.
    #[default]
    None,
    /// SQS `ReceiveMessage`: how many messages came back.
    Messages,
    /// SQS `SendMessage`: the size of the body sent.
    Sent,
    /// SQS `SendMessageBatch`: how many were sent, and how many failed.
    SentMessages,
    /// SQS `DeleteMessageBatch`: how many were deleted, and how many failed.
    DeletedMessages,
    /// S3 `DeleteObjects`: how many keys were deleted, and how many failed.
    DeletedKeys,
    /// S3 listings: how many keys, and whether more are left to list (`failed` is 1).
    Keys,
    /// An object's size, in bytes.
    Size,
    /// S3 `ListBuckets`.
    Buckets,
    /// DynamoDB: how many items.
    Items,
    /// Events put or read (Logs, EventBridge): how many, and how many failed.
    Events,
    /// Kinesis records put or read: how many, and how many failed.
    Records,
    /// SNS `Publish`: how many deliveries the bus made for it, and how many failed.
    Delivered,
    /// SNS: subscriptions listed.
    Subscriptions,
    /// SNS `Subscribe` answered `pending confirmation`.
    Pending,
    /// SNS `Subscribe` answered a subscription ARN: confirmed at once.
    Confirmed,
    /// Step Functions `GetActivityTask`: whether a task was handed over (`n` is 1) or the poll ended with none.
    Task,
    /// CloudWatch `PutMetricData`.
    Metrics,
    /// CloudWatch `GetMetricData`: the series answered.
    Series,
    /// STS: the session name it was asked for, held as the detail.
    Session,
    /// IAM: who or what an attach or a membership was for, held as the detail.
    To,
}

/// A result, compact: a kind and up to two counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub kind: Kind,
    /// How many of them failed (or, for a listing, 1 when more are left).
    pub failed: u16,
    /// The count, or a size in bytes.
    pub n: u32,
}

impl Summary {
    /// No summary.
    pub const NONE: Self = Self {
        kind: Kind::None,
        failed: 0,
        n: 0,
    };

    /// A summary of `kind` with its counts.
    #[must_use]
    pub const fn of(kind: Kind, n: u32, failed: u16) -> Self {
        Self { kind, failed, n }
    }

    /// Whether there is one.
    #[must_use]
    pub const fn is_some(self) -> bool {
        !matches!(self.kind, Kind::None)
    }

    /// The summary in words; `detail` is the short text some kinds carry (an index, a session name, a role). `None` for no summary.
    #[must_use]
    pub fn text(self, detail: &str) -> Option<String> {
        let n = self.n;
        let more = |one: &str, many: &str, none: &str| match n {
            0 => none.to_owned(),
            1 => format!("1 {one}"),
            _ => format!("{n} {many}"),
        };
        let failed = |s: String| {
            if self.failed > 0 {
                format!("{s}, {} failed", self.failed)
            } else {
                s
            }
        };
        let with = |s: String| {
            if detail.is_empty() {
                s
            } else {
                format!("{s} {detail}")
            }
        };
        Some(match self.kind {
            Kind::None => return None,
            Kind::Messages => more("message", "messages", "no messages"),
            Kind::Sent => format!("sent {}", size(n.into())),
            Kind::SentMessages => failed(format!("sent {}", more("message", "messages", "none"))),
            Kind::DeletedMessages => {
                failed(format!("deleted {}", more("message", "messages", "none")))
            }
            Kind::DeletedKeys => failed(format!("deleted {}", more("key", "keys", "none"))),
            Kind::Keys => {
                let s = more("key", "keys", "no keys");
                if self.failed > 0 {
                    format!("{s}, more to list")
                } else {
                    s
                }
            }
            Kind::Size => size(n.into()),
            Kind::Buckets => more("bucket", "buckets", "no buckets"),
            Kind::Items => with_via(more("item", "items", "no items"), detail),
            Kind::Events => failed(more("event", "events", "no events")),
            Kind::Records => failed(more("record", "records", "no records")),
            Kind::Delivered => match (n, self.failed) {
                (0, 0) => "no deliveries".to_owned(),
                (0, _) => failed("none delivered".to_owned()),
                _ => failed(format!("delivered to {n}")),
            },
            Kind::Subscriptions => more("subscription", "subscriptions", "no subscriptions"),
            Kind::Pending => "pending confirmation".to_owned(),
            Kind::Confirmed => "confirmed at once".to_owned(),
            Kind::Task => if n == 0 {
                "no task"
            } else {
                "task handed over"
            }
            .to_owned(),
            Kind::Metrics => more("metric", "metrics", "no metrics"),
            Kind::Series => more("series", "series", "no series"),
            Kind::Session => with("session".to_owned()),
            Kind::To => with("to".to_owned()),
        })
    }
}

fn with_via(s: String, detail: &str) -> String {
    if detail.is_empty() {
        s
    } else {
        format!("{s} via {detail}")
    }
}

/// A size in binary units, as the console writes one: `512 B`, `1.5 KiB`, `12 MiB`.
#[must_use]
pub fn size(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let (mut v, mut u) = (n as f64, 0);
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    let unit = UNITS.get(u).copied().unwrap_or("B");
    if u == 0 || v >= 10.0 {
        format!("{} {unit}", v.round())
    } else {
        format!("{v:.1} {unit}")
    }
}
