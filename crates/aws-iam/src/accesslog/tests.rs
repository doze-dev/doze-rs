#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use super::*;

fn seen(principal: &str, action: &str, resource: &str, at_ms: i64) -> Seen {
    Seen {
        principal: principal.into(),
        role: None,
        action: action.into(),
        resource: resource.into(),
        decision: "allowed",
        reason: "".into(),
        by: None,
        at_ms,
    }
}

#[test]
fn a_row_counts_repeats_and_keeps_the_latest_verdict() {
    let mut log = AccessLog::default();
    log.record(seen("u", "sqs:SendMessage", "q", 1000));
    let mut denied = seen("u", "sqs:SendMessage", "q", 2000);
    denied.decision = "implicitDeny";
    log.record(denied);
    let out = log.entries("soft");
    assert_eq!(out["Entries"].as_array().unwrap().len(), 1);
    assert_eq!(out["Entries"][0]["Count"], 2);
    assert_eq!(out["Entries"][0]["Decision"], "implicitDeny");
    assert_eq!(out["Entries"][0]["FirstSeen"], "1970-01-01T00:00:01.000Z");
}

#[test]
fn the_log_is_bounded_and_drops_the_least_recently_seen() {
    let mut log = AccessLog::default();
    for i in 0..MAX_ENTRIES + 5 {
        log.record(seen(
            "u",
            "s3:GetObject",
            &format!("r{i}"),
            i64::try_from(i).unwrap(),
        ));
    }
    assert_eq!(log.len(), MAX_ENTRIES);
    let out = log.entries("soft");
    assert_eq!(out["Evicted"], 5);
    assert!(
        !out["Entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["Resource"] == "r0")
    );
}

#[test]
fn a_repeat_keeps_its_row_over_older_ones_when_the_log_is_full() {
    let mut log = AccessLog::default();
    for i in 0..MAX_ENTRIES {
        log.record(seen("u", "s3:GetObject", &format!("r{i}"), 1));
    }
    // r0 is seen again: r1 is now the least recently seen, and goes first.
    log.record(seen("u", "s3:GetObject", "r0", 2));
    log.record(seen("u", "s3:GetObject", "new", 3));
    let out = log.entries("soft");
    let resources: Vec<&str> = out["Entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["Resource"].as_str().unwrap())
        .collect();
    assert!(resources.contains(&"r0") && resources.contains(&"new"));
    assert!(!resources.contains(&"r1"));
    assert_eq!(out["Evicted"], 1);
}

#[test]
fn the_roots_questions_are_one_row_each_however_often_asked() {
    let mut log = AccessLog::default();
    for at in 0..3 {
        log.note_root(
            "123456789012",
            [("sqs:SendMessage", "arn:q")].into_iter(),
            at,
        );
    }
    let out = log.entries("soft");
    assert_eq!(out["Entries"].as_array().unwrap().len(), 1);
    assert_eq!(
        out["Entries"][0]["Principal"],
        "arn:aws:iam::123456789012:root"
    );
    assert_eq!(out["Entries"][0]["Count"], 3);
    assert_eq!(
        out["Entries"][0]["Reason"],
        "the account root is allowed everything"
    );
}

/// `cargo test -p aws-iam --release accesslog_bench -- --ignored --nocapture`: new rows into a full log, and repeats.
#[test]
#[ignore = "a measurement, not a check"]
fn accesslog_bench() {
    let mut log = AccessLog::default();
    let t = std::time::Instant::now();
    for i in 0..50_000_i64 {
        log.record(seen(
            "arn:aws:iam::000000000000:root",
            "sqs:SendMessage",
            &format!("arn:aws:sqs:us-east-1:000000000000:q{i}"),
            i,
        ));
    }
    let fresh = t.elapsed();
    let t = std::time::Instant::now();
    for i in 0..200_000_i64 {
        log.record(seen(
            "arn:aws:iam::000000000000:root",
            "sqs:SendMessage",
            "arn:aws:sqs:us-east-1:000000000000:q49999",
            i,
        ));
    }
    println!(
        "50k new rows (full log): {fresh:?}; 200k repeats: {:?}",
        t.elapsed()
    );
}

#[test]
fn a_generated_policy_grants_what_was_used_and_names_what_it_could_not_scope() {
    let mut log = AccessLog::default();
    log.record(seen("alice", "sqs:SendMessage", "arn:q", 1));
    log.record(seen("alice", "sqs:ReceiveMessage", "arn:q", 2));
    log.record(seen("alice", "sqs:ListQueues", "*", 3));
    log.record(seen("bob", "s3:GetObject", "arn:b/k", 4));
    let mut session = seen("arn:sts/assumed-role/r/s", "kms:Decrypt", "arn:k", 5);
    session.role = Some("arn:role/r".into());
    log.record(session);

    let all = log.generate(Some("alice"), false).unwrap();
    assert_eq!(
        all["PolicyDocument"]["Statement"],
        json!([{"Effect": "Allow", "Action": ["sqs:ListQueues", "sqs:ReceiveMessage", "sqs:SendMessage"], "Resource": "*"}])
    );
    let scoped = log.generate(Some("alice"), true).unwrap();
    assert_eq!(scoped["StatementCount"], 2);
    assert_eq!(scoped["UnresolvedResources"], 1);
    assert_eq!(
        scoped["PolicyDocument"]["Statement"][1],
        json!({"Effect": "Allow", "Action": ["sqs:ReceiveMessage", "sqs:SendMessage"], "Resource": "arn:q"})
    );
    // A role's sessions are found by the role's ARN.
    let role = log.generate(Some("arn:role/r"), true).unwrap();
    assert_eq!(
        role["PolicyDocument"]["Statement"][0]["Action"],
        json!(["kms:Decrypt"])
    );
    assert!(log.generate(Some("nobody"), false).is_none());
    // The document is one IAM accepts.
    let text = all["PolicyDocument"].to_string();
    aws_iam_policy::check(&text, aws_iam_policy::Kind::Identity).unwrap();
}
