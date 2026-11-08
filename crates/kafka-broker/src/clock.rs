//! The broker's clock.

use std::time::{SystemTime, UNIX_EPOCH};

use kafka_wire::ids::TimestampMs;

/// Now, in milliseconds since the Unix epoch.
pub(crate) fn now() -> TimestampMs {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    TimestampMs::from_wire(ms)
}
