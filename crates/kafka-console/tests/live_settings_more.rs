//! The rule the Settings dialog teaches, hostile text on Settings, and a caller without rights: over a real broker in this process.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_access;
mod support_write;

use kafka_console::settings_words::DYNAMIC;

/// The smallest value a config takes, where the broker has one (what the test tries on it).
fn least(key: &str) -> Option<i64> {
    match key {
        "log.segment.bytes" => Some(1_048_576),
        "log.index.interval.bytes" | "message.max.bytes" => Some(0),
        "log.retention.ms" => Some(-1),
        _ => None,
    }
}
use support_access::{alter, seeded};

/// CON-569: the rule the dialog teaches is the one the broker holds: the least value is taken and one less is refused.
#[tokio::test]
async fn the_rule_the_dialog_teaches_is_the_rule_the_broker_holds() {
    let w = seeded("set-rule").await;
    for (key, _, _) in DYNAMIC {
        if let Some(n) = least(key) {
            assert_eq!(
                alter(&w, "", key, &n.to_string(), true).await.0,
                0,
                "{key} takes {n}"
            );
            if key != "log.retention.ms" || n > i64::MIN {
                assert_eq!(
                    alter(&w, "", key, &(n - 1).to_string(), true).await.0,
                    42,
                    "{key} does not take {}",
                    n - 1
                );
            }
        }
    }
    assert_eq!(
        alter(&w, "", "log.retention.bytes", "-5", true).await.0,
        0,
        "retention.bytes takes any number"
    );
    assert_eq!(
        alter(&w, "", "log.segment.bytes", "2147483648", true)
            .await
            .0,
        42,
        "the 32-bit ones stop at 2,147,483,647"
    );
    assert_eq!(
        alter(&w, "", "log.segment.bytes", "2147483647", true)
            .await
            .0,
        0
    );
    w.stop().await;
}
