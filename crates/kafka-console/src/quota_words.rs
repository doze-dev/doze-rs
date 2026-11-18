//! The words of the quotas on the Access page: a limit, a rate, and who a quota holds, in the console's voice. Pure: every
//! sentence is a function of its arguments.

use kafka_broker::bytes_words;

/// A value as `kafka-configs.sh` takes it and Kafka lists it: a whole number without a fraction, else the shortest decimal.
#[must_use]
pub fn value_text(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.0}")
    } else {
        format!("{v}")
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a rate in bytes a second, rounded down and never below zero"
)]
fn bytes_a_second(v: f64) -> String {
    format!("{}/s", bytes_words(v.max(0.0) as u64))
}

/// A number with at most one decimal: `12.5`, `3`.
fn short(v: f64) -> String {
    let tenths = (v * 10.0).round() / 10.0;
    value_text(tenths)
}

/// The limit a quota sets, in words.
#[must_use]
pub fn limit(key: &str, v: f64) -> String {
    match key {
        "producer_byte_rate" => format!("{} produced", bytes_a_second(v)),
        "consumer_byte_rate" => format!("{} fetched", bytes_a_second(v)),
        "request_percentage" => format!("{}% of a request thread", short(v)),
        "controller_mutation_rate" => format!("{} partitions made or deleted a second", short(v)),
        "connection_creation_rate" => format!("{} new connections a second", short(v)),
        other => format!("{other} {}", value_text(v)),
    }
}

/// A rate measured against a quota of `key`, in its unit.
#[must_use]
pub fn rate(key: &str, v: f64) -> String {
    match key {
        "producer_byte_rate" | "consumer_byte_rate" => bytes_a_second(v),
        "request_percentage" => format!("{}% of a thread", short(v)),
        _ => format!("{}/s", short(v)),
    }
}

/// Who a quota of an entity holds, from its parts (`None`: the default of the type): Kafka's precedence in one sentence.
#[must_use]
pub fn applies(
    user: Option<Option<&str>>,
    client: Option<Option<&str>>,
    ip: Option<Option<&str>>,
) -> String {
    match (user, client, ip) {
        (_, _, Some(Some(addr))) => format!("connections from {addr} (kept and listed; this broker does not enforce address quotas)"),
        (_, _, Some(None)) => "connections from any address without its own quota (kept and listed; not enforced here)".to_owned(),
        (Some(Some(u)), Some(Some(c)), _) => format!("client id {c} of user {u}"),
        (Some(Some(u)), Some(None), _) => format!("each client id of user {u} on its own, unless that client id has its own"),
        (Some(Some(u)), None, _) => format!("every client of user {u}, together, whatever their client ids"),
        (Some(None), Some(Some(c)), _) => format!("client id {c} of each user without a quota for it"),
        (Some(None), Some(None), _) => "each client id of each user, on its own, where nothing more specific is set".to_owned(),
        (Some(None), None, _) => "each user without a quota of their own, each on their own".to_owned(),
        (None, Some(Some(c)), _) => format!("every client with client id {c}, together, unless its user has a quota"),
        (None, Some(None), _) => "each client id without a quota of its own, on its own (a client that sends none is not held)".to_owned(),
        (None, None, None) => "nobody".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_as_kafka_configs_writes_them() {
        assert_eq!(value_text(1_048_576.0), "1048576");
        assert_eq!(value_text(12.5), "12.5");
        assert_eq!(value_text(0.25), "0.25");
    }

    #[test]
    fn a_limit_says_its_unit() {
        assert_eq!(
            limit("producer_byte_rate", 1_048_576.0),
            "1.0 MiB/s produced"
        );
        assert_eq!(limit("consumer_byte_rate", 100.0), "100 B/s fetched");
        assert_eq!(
            limit("request_percentage", 12.5),
            "12.5% of a request thread"
        );
        assert_eq!(
            limit("controller_mutation_rate", 3.0),
            "3 partitions made or deleted a second"
        );
        assert_eq!(
            limit("connection_creation_rate", 50.0),
            "50 new connections a second"
        );
        assert_eq!(rate("request_percentage", 0.04), "0% of a thread");
        assert_eq!(rate("controller_mutation_rate", 2.25), "2.3/s");
    }

    #[test]
    fn who_a_quota_holds_follows_kafkas_precedence() {
        assert_eq!(
            applies(Some(Some("alice")), None, None),
            "every client of user alice, together, whatever their client ids"
        );
        assert_eq!(
            applies(None, Some(Some("c1")), None),
            "every client with client id c1, together, unless its user has a quota"
        );
        assert_eq!(
            applies(Some(Some("alice")), Some(Some("c1")), None),
            "client id c1 of user alice"
        );
        assert!(applies(None, Some(None), None).contains("a client that sends none is not held"));
        assert!(applies(None, None, Some(Some("10.0.0.1"))).contains("does not enforce"));
    }
}
