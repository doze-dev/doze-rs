use kafka_auth::authz::{operation, resource};

use super::*;

fn fields<'a>(kind: &'a str, name: &'a str, principal: &'a str) -> AclFields<'a> {
    AclFields {
        kind,
        name,
        pattern: "literal",
        principal,
        host: "*",
        op: "write",
        perm: "allow",
    }
}

fn plain(lines: &Lines) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.iter().map(|(_, t)| t.as_str()).collect())
        .collect()
}

#[test]
fn a_form_is_read_word_by_word_and_a_word_that_is_none_of_the_list_reads_as_the_first() {
    let i = input_of(&AclFields {
        kind: "nonsense",
        name: "  orders ",
        pattern: "x",
        principal: "User:a",
        host: "",
        op: "",
        perm: "?",
    });
    assert_eq!(
        (
            i.kind.as_str(),
            i.name.as_str(),
            i.pattern.as_str(),
            i.host.as_str(),
            i.operation.as_str(),
            i.permission.as_str()
        ),
        ("topic", "orders", "literal", "*", "read", "allow")
    );
    let e = entry_of(&input_of(&fields("transactional-id", "t", "User:x")));
    assert_eq!(
        (e.kind, e.operation),
        (resource::TRANSACTIONAL_ID, operation::WRITE)
    );
}

#[test]
fn the_cluster_has_one_name_and_an_empty_field_means_it() {
    let e = entry_of(&input_of(&fields("cluster", "", "User:x")));
    assert_eq!(e.name, "kafka-cluster");
    let t = entry_of(&input_of(&fields("topic", "", "User:x")));
    assert_eq!(t.name, "");
    assert_eq!(missing(&t).unwrap().0, "name");
    assert_eq!(
        missing(&entry_of(&input_of(&fields("topic", "a", ""))))
            .unwrap()
            .0,
        "principal"
    );
    assert_eq!(
        missing(&entry_of(&input_of(&fields("topic", "a", "User:x")))),
        None
    );
}

#[test]
fn a_text_is_cut_to_a_length_no_acl_needs() {
    let long = "x".repeat(5_000);
    let i = input_of(&fields("topic", &long, "User:x"));
    assert_eq!(i.name.chars().count(), MOST_TEXT);
}

#[test]
fn the_calls_are_as_the_boards_draw_them() {
    let e = entry_of(&input_of(&fields("topic", "orders", "User:orders")));
    assert_eq!(
        plain(&create_call(3, &e)),
        [
            "CreateAcls v3",
            "  creations[0]",
            "    resource_type: TOPIC",
            "    resource_name: orders",
            "    resource_pattern_type: LITERAL",
            "    principal: User:orders",
            "    host: *",
            "    operation: WRITE",
            "    permission_type: ALLOW",
        ]
    );
    assert_eq!(
        plain(&delete_call(3, &e)),
        [
            "DeleteAcls v3",
            "  filters[0]",
            "    resource_type_filter: TOPIC",
            "    resource_name_filter: orders",
            "    pattern_type_filter: LITERAL",
            "    principal_filter: User:orders",
            "    host_filter: *",
            "    operation: WRITE",
            "    permission_type: ALLOW",
        ]
    );
}

#[test]
fn what_was_typed_cannot_start_a_line_of_the_call() {
    let e = entry_of(&input_of(&fields(
        "topic",
        "a\n    operation: ALL",
        "User:x\r\nhost: evil",
    )));
    let call = create_call(3, &e);
    assert_eq!(call.len(), 9, "{call:?}");
    assert!(plain(&call)[3].contains("\\n"), "{call:?}");
    assert!(plain(&call)[5].contains("\\r\\n"), "{call:?}");
}
