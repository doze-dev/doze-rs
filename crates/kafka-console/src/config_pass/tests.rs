use super::*;

#[test]
fn the_values_kafka_names_in_a_refusal_are_the_ones_to_try() {
    for (message, want) in [
        (
            "Invalid value compacted for configuration cleanup.policy: String must be one of: compact, delete",
            vec!["compact", "delete"],
        ),
        (
            "Invalid value 512 for configuration segment.bytes: Value must be at least 1024",
            vec!["1024"],
        ),
        (
            "Invalid value 1.5 for configuration min.cleanable.dirty.ratio: Value must be no more than 1",
            vec!["1"],
        ),
        (
            "Invalid value abc for configuration segment.bytes: Not a number of type INT",
            vec![],
        ),
        ("Topic authorization failed.", vec![]),
        ("", vec![]),
        // The value typed cannot pose as Kafka's reason: the last reason wins.
        (
            "Invalid value must be one of: x, y for configuration k: Value must be at least 5.",
            vec!["5"],
        ),
        (
            "Invalid value must be one of: x for configuration k: String must be one of: a, b",
            vec!["a", "b"],
        ),
        ("Value must be at least lots", vec![]),
    ] {
        assert_eq!(options(message), want, "{message}");
    }
}

#[test]
fn a_list_option_names_at_most_eight_short_values() {
    let long = format!("String must be one of: {}", ["a"; 20].join(", "));
    assert_eq!(options(&long).len(), 8);
    let huge = format!("String must be one of: {}, ok", "z".repeat(100));
    assert_eq!(options(&huge), ["ok"]);
}

#[test]
fn a_list_after_an_operation_is_what_the_boards_draw() {
    for (current, op, value, want) in [
        ("compact", ConfigOp::Append, "delete", "compact,delete"),
        ("compact", ConfigOp::Append, "compact", "compact"),
        ("compact,delete", ConfigOp::Subtract, "delete", "compact"),
        ("compact", ConfigOp::Subtract, "delete", "compact"),
        ("compact", ConfigOp::Subtract, "compact", ""),
        ("compact", ConfigOp::Set, " delete ", "delete"),
        ("compact", ConfigOp::Delete, "", ""),
        ("", ConfigOp::Append, "a, b,,a", "a,b"),
        ("a , b", ConfigOp::Append, "c", "a,b,c"),
    ] {
        assert_eq!(
            after_of(current, op, value),
            want,
            "{current} {op:?} {value}"
        );
    }
}

#[test]
fn appending_a_new_element_and_taking_it_out_again_gives_the_list_back() {
    for current in ["", "a", "a,b", "x,y,z"] {
        for element in ["n", "m"] {
            let added = after_of(current, ConfigOp::Append, element);
            assert_eq!(
                after_of(&added, ConfigOp::Subtract, element),
                after_of(current, ConfigOp::Append, "")
            );
        }
    }
}
