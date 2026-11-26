//! What the Lab's tests share: a clock the test moves, a table on it, and the shortest way to a request and a rule.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use aws_gateway::Ask;

use super::rule::{Effect, Fail, Matcher, Share, Spec};
use super::table::{Clock, LabTable};

/// A clock that moves when the test says so.
#[derive(Debug, Clone, Default)]
pub(crate) struct Manual(pub(crate) Arc<AtomicU64>);

impl Manual {
    pub(crate) fn advance(&self, d: Duration) {
        self.0
            .fetch_add(u64::try_from(d.as_millis()).unwrap_or(0), Ordering::SeqCst);
    }
}

impl Clock for Manual {
    fn mono_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    fn wall_ms(&self) -> i64 {
        1_800_000_000_000 + i64::try_from(self.0.load(Ordering::SeqCst)).unwrap_or(0)
    }
}

/// A table on a clock of its own, with the seed of the test.
pub(crate) fn table() -> (Arc<LabTable>, Manual) {
    let clock = Manual::default();
    (LabTable::with(Box::new(clock.clone()), 7), clock)
}

/// A call of `op` to the service `signing`.
pub(crate) fn ask<'a>(signing: &'static str, sdk_id: &'static str, op: &'a str) -> Ask<'a> {
    Ask {
        signing_name: signing,
        sdk_id,
        operation: op,
        resource: "",
        access_key_id: "AKIDLAB",
        region: "us-east-1",
        user_agent: "Boto3/1.34.69 md/Botocore#1.34.69",
    }
}

/// A `SendMessage` to the queue `orders`.
pub(crate) fn send<'a>() -> Ask<'a> {
    Ask {
        resource: "http://localhost:4566/000000000000/orders",
        ..ask("sqs", "SQS", "SendMessage")
    }
}

/// A rule that fits every call and throttles it, for ten minutes.
pub(crate) fn throttle() -> Spec {
    Spec {
        name: String::new(),
        matcher: Matcher::default(),
        effect: Effect::Fail(Fail::Throttle),
        share: Share::Every,
        ttl: Duration::from_secs(600),
        max_hits: None,
    }
}

/// A rule that fits every call and delays it by `ms`.
pub(crate) fn slow(ms: u32) -> Spec {
    Spec {
        effect: Effect::Slow {
            delay_ms: ms,
            jitter_ms: 0,
        },
        ..throttle()
    }
}

/// A rule that fits the calls to the service `word` and throttles them.
pub(crate) fn throttle_of(word: &str) -> Spec {
    let (matcher, said) = Matcher::build(&[word.to_owned()], &[], &[], &[], &[]);
    assert!(said.is_empty());
    Spec {
        matcher,
        ..throttle()
    }
}
