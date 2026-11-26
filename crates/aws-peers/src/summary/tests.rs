#![allow(clippy::unwrap_used)]

use serde_json::json;

use super::*;

fn said(kind: Kind, n: u32, failed: u16, detail: &str) -> String {
    Summary::of(kind, n, failed).text(detail).unwrap()
}

#[test]
fn counts_are_said_in_the_singular_and_the_plural() {
    assert_eq!(said(Kind::Messages, 0, 0, ""), "no messages");
    assert_eq!(said(Kind::Messages, 1, 0, ""), "1 message");
    assert_eq!(said(Kind::Messages, 7, 0, ""), "7 messages");
    assert_eq!(said(Kind::Keys, 6, 0, ""), "6 keys");
    assert_eq!(said(Kind::Keys, 1000, 1, ""), "1000 keys, more to list");
    assert_eq!(said(Kind::Events, 2, 1, ""), "2 events, 1 failed");
    assert_eq!(said(Kind::SentMessages, 3, 0, ""), "sent 3 messages");
    assert_eq!(said(Kind::DeletedKeys, 0, 2, ""), "deleted none, 2 failed");
    assert_eq!(
        said(Kind::Items, 3, 0, "by-customer"),
        "3 items via by-customer"
    );
    assert_eq!(said(Kind::Items, 1, 0, ""), "1 item");
    assert_eq!(said(Kind::Series, 2, 0, ""), "2 series");
}

#[test]
fn deliveries_are_said_only_as_counted() {
    assert_eq!(said(Kind::Delivered, 3, 0, ""), "delivered to 3");
    assert_eq!(said(Kind::Delivered, 2, 1, ""), "delivered to 2, 1 failed");
    assert_eq!(said(Kind::Delivered, 0, 0, ""), "no deliveries");
    assert_eq!(said(Kind::Delivered, 0, 1, ""), "none delivered, 1 failed");
}

#[test]
fn text_kinds_carry_their_detail_and_none_says_nothing() {
    assert_eq!(
        said(Kind::Session, 0, 0, "report-job"),
        "session report-job"
    );
    assert_eq!(said(Kind::To, 0, 0, "dev"), "to dev");
    assert_eq!(said(Kind::Task, 0, 0, ""), "no task");
    assert_eq!(said(Kind::Task, 1, 0, ""), "task handed over");
    assert_eq!(Summary::NONE.text(""), None);
    assert!(!Summary::NONE.is_some());
}

#[test]
fn sizes_read_as_the_console_writes_them() {
    for (n, words) in [
        (0, "0 B"),
        (53, "53 B"),
        (1536, "1.5 KiB"),
        (212 * 1024, "212 KiB"),
        (12 << 20, "12 MiB"),
        (u64::from(u32::MAX), "4.0 GiB"),
    ] {
        assert_eq!(size(n), words);
    }
    assert_eq!(said(Kind::Sent, 53, 0, ""), "sent 53 B");
}

fn output(sdk: &str, action: &str, answer: &serde_json::Value) -> Summary {
    let mut s = Summary::NONE;
    from_output(sdk, action, answer, &mut s);
    s
}

#[test]
fn a_rest_answer_is_read_by_member_name_whatever_its_case() {
    let list =
        json!({"Contents": [{"Key": "a"}, {"Key": "b"}], "IsTruncated": true, "KeyCount": 2});
    assert_eq!(
        output("S3", "ListObjectsV2", &list),
        Summary::of(Kind::Keys, 2, 1)
    );
    // An empty listing omits its list: it has no keys, and says so.
    assert_eq!(
        output("S3", "ListObjectsV2", &json!({"IsTruncated": false})),
        Summary::of(Kind::Keys, 0, 0)
    );
    assert_eq!(
        output("S3", "GetObject", &json!({"ContentLength": 212})),
        Summary::of(Kind::Size, 212, 0)
    );
    // A size the answer does not carry is not guessed.
    assert_eq!(output("S3", "GetObject", &json!({})), Summary::NONE);
    assert_eq!(
        output(
            "CloudWatch Logs",
            "GetLogEvents",
            &json!({"events": [{}, {}]})
        ),
        Summary::of(Kind::Events, 2, 0)
    );
}

#[test]
fn an_operation_the_table_does_not_know_has_no_summary() {
    assert_eq!(output("SQS", "PurgeQueue", &json!({})), Summary::NONE);
    assert_eq!(
        output("S3", "ListObjectsV2x", &json!({"Contents": []})),
        Summary::NONE
    );
    // The same operation name in another API is another operation.
    assert_eq!(
        output("Lambda", "ReceiveMessage", &json!({"Messages": [{}]})),
        Summary::NONE
    );
}

#[test]
fn a_subscription_that_needs_no_confirmation_is_said_so() {
    assert_eq!(
        output(
            "SNS",
            "Subscribe",
            &json!({"SubscriptionArn": "pending confirmation"})
        )
        .kind,
        Kind::Pending
    );
    assert_eq!(
        output(
            "SNS",
            "Subscribe",
            &json!({"SubscriptionArn": "arn:aws:sns:us-east-1:0:t:1"})
        )
        .kind,
        Kind::Confirmed
    );
}

#[test]
fn a_detail_is_cut_on_a_character_boundary() {
    let long = "é".repeat(80);
    let mut detail = String::new();
    super::table::from_input(
        "STS",
        "AssumeRole",
        &json!({"RoleSessionName": long}),
        &mut Summary::default(),
        &mut detail,
    );
    assert_eq!(detail.len(), MOST_DETAIL);
    assert!(detail.chars().all(|c| c == 'é'));
}

#[test]
fn iam_lists_are_counted_in_the_words_of_what_they_list() {
    let words = |action: &str, answer: &serde_json::Value| {
        output("IAM", action, answer).text("").unwrap_or_default()
    };
    assert_eq!(
        words("ListGroupsForUser", &json!({"Groups": [{}]})),
        "1 group"
    );
    assert_eq!(
        words("ListGroupsForUser", &json!({"Groups": [{}, {}]})),
        "2 groups"
    );
    assert_eq!(words("ListGroupsForUser", &json!({})), "no groups");
    for action in [
        "ListAttachedUserPolicies",
        "ListAttachedGroupPolicies",
        "ListAttachedRolePolicies",
    ] {
        assert_eq!(
            words(action, &json!({"AttachedPolicies": [{}]})),
            "1 policy",
            "{action}"
        );
        assert_eq!(words(action, &json!({})), "no policies", "{action}");
    }
    for action in ["ListUserPolicies", "ListGroupPolicies", "ListRolePolicies"] {
        assert_eq!(
            words(action, &json!({"PolicyNames": ["a", "b"]})),
            "2 policies",
            "{action}"
        );
    }
    assert_eq!(
        words("ListAccessKeys", &json!({"AccessKeyMetadata": [{}]})),
        "1 access key"
    );
    assert_eq!(
        words("ListMFADevices", &json!({"MFADevices": [{}, {}]})),
        "2 MFA devices"
    );
    assert_eq!(
        words("ListPolicyVersions", &json!({"Versions": [{}, {}, {}]})),
        "3 versions"
    );
}

#[test]
fn the_entities_of_a_policy_are_its_users_groups_and_roles_together() {
    let answer = json!({"PolicyUsers": [{}], "PolicyGroups": [{}], "PolicyRoles": [{}, {}]});
    assert_eq!(
        output("IAM", "ListEntitiesForPolicy", &answer).text(""),
        Some("4 entities".to_owned())
    );
    assert_eq!(
        output(
            "IAM",
            "ListEntitiesForPolicy",
            &json!({"PolicyUsers": [{}]})
        )
        .text(""),
        Some("1 entity".to_owned())
    );
    assert_eq!(
        output("IAM", "ListEntitiesForPolicy", &json!({})).text(""),
        Some("no entities".to_owned())
    );
}

#[test]
fn a_policy_version_read_is_said_by_its_number_and_nothing_is_guessed() {
    let read = |id: serde_json::Value| {
        output(
            "IAM",
            "GetPolicyVersion",
            &json!({"PolicyVersion": {"VersionId": id, "IsDefaultVersion": true}}),
        )
    };
    assert_eq!(read(json!("v1")).text(""), Some("v1".to_owned()));
    assert_eq!(read(json!("v12")).text(""), Some("v12".to_owned()));
    assert_eq!(read(json!("latest")), Summary::NONE);
    assert_eq!(output("IAM", "GetPolicyVersion", &json!({})), Summary::NONE);
    // Another API's operation of the same name is not an IAM one.
    assert_eq!(
        output("SQS", "ListGroupsForUser", &json!({"Groups": [{}]})),
        Summary::NONE
    );
    // An IAM operation the table does not know has no summary.
    assert_eq!(
        output("IAM", "GetUser", &json!({"User": {}})),
        Summary::NONE
    );
}
