//! The names the pages use: an API's name as a `&'static str`, and the short tag of a client library.

use std::sync::LazyLock;

use kafka_wire::messages::ApiKey;

/// Every API's name, made once: `ApiKey` prints as its Kafka name.
static NAMES: LazyLock<Vec<(i16, &'static str)>> = LazyLock::new(|| {
    ApiKey::iter()
        .map(|k| (k as i16, &*Box::leak(format!("{k:?}").into_boxed_str())))
        .collect()
});

/// The name of an API (`Produce`, `OffsetCommit`).
#[must_use]
pub fn api(key: ApiKey) -> &'static str {
    NAMES
        .iter()
        .find(|(k, _)| *k == key as i16)
        .map_or("Unknown", |(_, n)| *n)
}

/// The API a name is for, if it is one.
#[must_use]
pub fn api_of(name: &str) -> Option<ApiKey> {
    NAMES
        .iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(name))
        .and_then(|(k, _)| ApiKey::try_from(*k).ok())
}

/// The short tag a client library is shown with, from the software name its ApiVersions request gave. Known libraries get
/// their language (`go`, `py`, `java`, `js`, `c`, `rs`, `net`, `ruby`, `php`); `kcat` and the console tools their own name; any other
/// software is its first word, cut short. `None` when the client said nothing.
#[must_use]
pub fn tag(software: &str) -> Option<String> {
    let s = software.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    let known: &[(&str, &str)] = &[
        ("apache-kafka-java", "java"),
        ("kafka-java", "java"),
        ("sarama", "go"),
        ("franz-go", "go"),
        ("kafka-go", "go"),
        ("confluent-kafka-go", "go"),
        ("kafkajs", "js"),
        ("kafka-python", "py"),
        ("confluent-kafka-python", "py"),
        ("aiokafka", "py"),
        ("rdkafka", "rs"),
        ("librdkafka", "c"),
        ("rust-rdkafka", "rs"),
        ("kafka-rust", "rs"),
        ("confluent.kafka", "net"),
        ("ruby-kafka", "ruby"),
        ("rdkafka-ruby", "ruby"),
        ("kcat", "kcat"),
        ("kafkacat", "kcat"),
    ];
    // The most specific name first: a longer name wins over one it contains.
    let mut best: Option<(&str, &str)> = None;
    for (name, tag) in known {
        if s.contains(name) && best.is_none_or(|(n, _)| name.len() > n.len()) {
            best = Some((name, tag));
        }
    }
    Some(best.map_or_else(
        || {
            s.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '.')
                .next()
                .unwrap_or("")
                .chars()
                .take(8)
                .collect()
        },
        |(_, t)| t.to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_names_are_kafkas() {
        assert_eq!(api(ApiKey::Produce), "Produce");
        assert_eq!(
            api(ApiKey::ConsumerGroupHeartbeat),
            "ConsumerGroupHeartbeat"
        );
        assert_eq!(api_of("produce"), Some(ApiKey::Produce));
        assert_eq!(api_of("nope"), None);
    }

    #[test]
    fn tags_name_the_language_of_known_libraries_and_say_nothing_for_silence() {
        assert_eq!(tag("sarama").as_deref(), Some("go"));
        assert_eq!(tag("franz-go").as_deref(), Some("go"));
        assert_eq!(tag("confluent-kafka-python").as_deref(), Some("py"));
        assert_eq!(tag("apache-kafka-java").as_deref(), Some("java"));
        assert_eq!(tag("librdkafka").as_deref(), Some("c"));
        assert_eq!(tag("rust-rdkafka").as_deref(), Some("rs"));
        assert_eq!(tag("kcat").as_deref(), Some("kcat"));
        assert_eq!(tag("somethingelse 1.0").as_deref(), Some("somethin"));
        assert_eq!(tag(""), None);
    }
}
