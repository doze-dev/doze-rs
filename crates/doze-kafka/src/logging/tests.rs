//! The filter reads `RUST_LOG`'s level and `target=level` forms as `EnvFilter` did, and refuses the span and field forms.

use tracing::Level;

use super::*;

fn on(f: &Targets, target: &str, level: Level) -> bool {
    f.would_enable(target, &level)
}

#[test]
fn a_bare_level_is_the_level_of_every_target() {
    let f = filter("info").unwrap();
    assert!(on(&f, "kafka_broker", Level::INFO) && on(&f, "doze_kafka", Level::WARN));
    assert!(!on(&f, "kafka_broker", Level::DEBUG));
    let off = filter("off").unwrap();
    assert!(!on(&off, "kafka_broker", Level::ERROR));
}

#[test]
fn a_target_level_wins_over_the_default_for_that_target_and_its_modules() {
    let f = filter("warn,kafka_broker=debug").unwrap();
    assert!(on(&f, "kafka_broker", Level::DEBUG));
    assert!(on(&f, "kafka_broker::fetch", Level::DEBUG));
    assert!(!on(&f, "kafka_broker", Level::TRACE));
    assert!(on(&f, "kafka_server", Level::WARN) && !on(&f, "kafka_server", Level::INFO));
}

#[test]
fn targets_alone_leave_every_other_target_silent_and_a_bare_target_is_every_level() {
    let f = filter("kafka_server=info").unwrap();
    assert!(on(&f, "kafka_server", Level::INFO));
    assert!(!on(&f, "kafka_broker", Level::ERROR));
    let bare = filter("kafka_wire").unwrap();
    assert!(on(&bare, "kafka_wire", Level::TRACE));
    assert!(!on(&bare, "kafka_broker", Level::ERROR));
}

#[test]
fn spaces_and_empty_directives_are_ignored() {
    let f = filter(" warn , , kafka_server = trace ,").unwrap();
    assert!(on(&f, "kafka_server", Level::TRACE));
    assert!(on(&f, "doze_kafka", Level::WARN) && !on(&f, "doze_kafka", Level::INFO));
}

#[test]
fn levels_are_read_in_any_case() {
    let f = filter("DEBUG,kafka_wire=Trace").unwrap();
    assert!(on(&f, "kafka_broker", Level::DEBUG) && on(&f, "kafka_wire", Level::TRACE));
}

#[test]
fn span_and_field_filters_are_refused_by_name() {
    for spec in [
        "kafka_broker[fetch]=debug",
        "[produce{topic=orders}]=trace",
        "info,kafka_server[{peer}]=debug",
        "kafka_server[{peer=10.0.0.1}]",
    ] {
        let why = filter(spec).unwrap_err();
        assert!(why.contains("span and field filters"), "{spec}: {why}");
    }
}

#[test]
fn a_directive_that_is_not_a_level_or_names_no_target_is_refused() {
    assert!(
        filter("kafka_broker=loud")
            .unwrap_err()
            .contains("loud is not a level")
    );
    assert!(filter("=debug").unwrap_err().contains("names no target"));
    assert!(filter("a=b=c").unwrap_err().contains("is not a level"));
}

#[test]
fn the_refusal_is_one_line_naming_its_source_and_what_is_supported() {
    let why = parse("RUST_LOG", "kafka_broker[fetch]=debug").unwrap_err();
    assert_eq!(why.lines().count(), 1, "{why}");
    assert!(
        why.starts_with("RUST_LOG \"kafka_broker[fetch]=debug\": span and field filters"),
        "{why}"
    );
    assert!(
        why.contains("target=level") && why.contains("warn,kafka_broker=debug"),
        "{why}"
    );
}
