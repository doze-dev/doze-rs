//! A share group's `share.auto.offset.reset`: Kafka's three forms, the third an ISO-8601 duration as Java parses it.

use super::Reset;

/// `earliest`, `latest` or `by_duration:<ISO-8601 duration>` (Kafka's `ShareGroupAutoOffsetResetStrategy`).
#[must_use]
pub(crate) fn parse_reset(v: &str) -> Option<Reset> {
    let v = v.trim().to_ascii_lowercase();
    match v.as_str() {
        "earliest" => Some(Reset::Earliest),
        "latest" => Some(Reset::Latest),
        other => other
            .strip_prefix("by_duration:")
            .and_then(iso_duration_ms)
            .map(Reset::ByDuration),
    }
}

/// An ISO-8601 duration as Java's `Duration.parse` takes it, `PnDTnHnMn.nS`, to milliseconds; negatives are refused.
fn iso_duration_ms(text: &str) -> Option<i64> {
    let rest = text.strip_prefix('p')?;
    let (days, time) = match rest.split_once('t') {
        Some((d, t)) => (d, Some(t)),
        None => (rest, None),
    };
    let mut ms: i64 = 0;
    if !days.is_empty() {
        let n: i64 = days.strip_suffix('d')?.parse().ok()?;
        ms = ms.checked_add(n.checked_mul(86_400_000)?)?;
    }
    let Some(mut time) = time else {
        return (!days.is_empty() && ms >= 0).then_some(ms);
    };
    if time.is_empty() {
        return None;
    }
    for (unit, scale) in [('h', 3_600_000_f64), ('m', 60_000_f64), ('s', 1_000_f64)] {
        if let Some(i) = time.find(unit) {
            let n: f64 = time.get(..i)?.parse().ok()?;
            if n < 0.0 || (unit != 's' && n.fract() != 0.0) {
                return None;
            }
            // Whole milliseconds: a fraction of one is dropped, as Java's toMillis does.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "checked to be a non-negative finite number of ms"
            )]
            let part = (n * scale) as i64;
            ms = ms.checked_add(part)?;
            time = time.get(i + 1..)?;
        }
    }
    (time.is_empty() && ms >= 0).then_some(ms)
}
