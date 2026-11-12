use super::*;

#[test]
fn kafkas_names_are_made_from_the_variants() {
    for (code, name) in [
        (3, "UNKNOWN_TOPIC_OR_PARTITION"),
        (10, "MESSAGE_TOO_LARGE"),
        (17, "INVALID_TOPIC_EXCEPTION"),
        (29, "TOPIC_AUTHORIZATION_FAILED"),
        (36, "TOPIC_ALREADY_EXISTS"),
        (37, "INVALID_PARTITIONS"),
        (40, "INVALID_CONFIG"),
        (44, "POLICY_VIOLATION"),
        (87, "INVALID_RECORD"),
        (1, "OFFSET_OUT_OF_RANGE"),
        (-1, "UNKNOWN_SERVER_ERROR"),
        (100, "UNKNOWN_TOPIC_ID"),
        (0, "NONE"),
        (9999, "UNKNOWN_ERROR_9999"),
    ] {
        assert_eq!(code_name(code), name, "{code}");
    }
}

#[test]
fn the_sentence_is_the_brokers_own_or_kafkas_description() {
    let r = refusal(36, Some("Topic 'orders.retry' already exists."));
    assert_eq!(
        (r.code, r.name.as_str(), r.message.as_str()),
        (
            36,
            "TOPIC_ALREADY_EXISTS",
            "Topic 'orders.retry' already exists."
        )
    );
    assert_eq!(
        refusal(44, None).message,
        "Request parameters do not satisfy the configured policy."
    );
    assert_eq!(refusal(44, Some("  ")).message, description(44));
    assert_eq!(refusal(44, Some("")).message, description(44));
}

#[test]
fn every_code_a_topic_write_can_answer_has_a_description_of_its_own() {
    let generic = description(-1);
    for code in [
        1, 2, 3, 7, 10, 17, 21, 29, 31, 32, 36, 37, 38, 39, 40, 42, 44, 56, 73, 76, 87, 100,
    ] {
        assert_ne!(description(code), generic, "{code} has no description");
    }
}

#[test]
fn a_reading_is_given_only_where_it_is_true() {
    assert!(create(36, None, None).would_pass.is_some());
    assert!(create(17, None, None).rule.is_some());
    assert_eq!(
        create(37, None, Some(1000)).would_pass.unwrap(),
        vec![Span::text("a count from 1 to 1,000.")]
    );
    assert!(
        create(7, None, None).rule.is_none(),
        "a timeout has no rule of the console's"
    );
    let keyless = produce(87, None, true, true, None);
    assert!(
        keyless
            .would_pass
            .unwrap()
            .iter()
            .any(|s| matches!(s, Span::Text(t) if t.contains("tombstone")))
    );
    let other = produce(87, None, false, false, None);
    assert!(other.rule.is_some() && other.would_pass.is_none());
    let large = produce(10, None, false, false, Some(1_048_588));
    assert_eq!(
        large.would_pass.unwrap(),
        vec![Span::text(
            "a record under 1,048,588 bytes (max.message.bytes), or a larger limit on the topic."
        )]
    );
    assert_eq!(
        add_partitions(37, None, 3, Some(1000)).would_pass.unwrap(),
        vec![Span::text("any total from 4 to 1,000.")]
    );
    assert!(delete_records(44, None).would_pass.is_some());
    assert_eq!(
        delete_records(1, Some(16_104)).would_pass.unwrap(),
        vec![Span::text("-1, or an offset from 0 to 16,104.")]
    );
    assert!(delete_topic(3, None).rule.is_some());
}

#[test]
fn an_authorization_refusal_names_the_principal_and_the_way_out() {
    for r in [
        create(29, None, None),
        produce(29, None, false, false, None),
        add_partitions(29, None, 1, None),
        delete_records(29, None),
        delete_topic(29, None),
    ] {
        assert_eq!(r.name, "TOPIC_AUTHORIZATION_FAILED");
        let rule = format!("{:?}", r.rule.unwrap());
        assert!(rule.contains("User:__console"), "{rule}");
        assert!(r.would_pass.is_some());
    }
}
