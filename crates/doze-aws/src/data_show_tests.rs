#![allow(clippy::unwrap_used)]

use serde_json::json;

use super::*;

fn report() -> Value {
    json!({
        "data_dir": "/x",
        "bytes": 3_u64 << 30,
        "services": [
            {"name": "sqs", "title": "SQS", "bytes": 90_u64 << 20, "one": "queue", "many": "queues", "units": "messages", "prunes": ["before"],
             "files": [{"region": "us-east-1", "bytes": 90_u64 << 20, "reclaimable": 2_u64 << 20}],
             "resources": [
                {"region": "us-east-1", "name": "jobs", "bytes": 1000, "items": 3, "notes": []},
                {"region": "us-east-1", "name": "orders-dlq", "bytes": 86_u64 << 20, "items": 212_418,
                 "notes": [{"bytes": 86_u64 << 20, "text": "dead-letter queue: nothing receives from it until someone redrives it", "prune": null}]}
             ]},
            {"name": "s3", "title": "S3", "bytes": 2_u64 << 30, "one": "bucket", "many": "buckets", "units": "objects", "prunes": ["before", "prefix", "old-versions"],
             "files": [],
             "resources": [
                {"region": "us-east-1", "name": "uploads", "bytes": 2_u64 << 30, "items": 41_203,
                 "notes": [{"bytes": 1_u64 << 30, "text": "in 3,120 old versions", "prune": {"old_versions": true}}]}
             ]},
            {"name": "iam", "title": "IAM", "bytes": 0, "one": null, "many": null, "units": null, "prunes": null, "files": [], "resources": []}
        ]
    })
}

#[test]
fn sizes_and_counts_read_as_a_person_reads_them() {
    assert_eq!(size(812), "812 B");
    assert_eq!(size(4096), "4.0 KB");
    assert_eq!(size(86 << 20), "86 MB");
    assert_eq!(size((42 << 30) / 10), "4.2 GB");
    assert_eq!(count(212_418), "212,418");
    assert_eq!(count(7), "7");
    assert_eq!(count(1_000), "1,000");
}

#[test]
fn the_overview_puts_the_largest_first_and_says_what_removes_the_hidden_parts() {
    let text = overview(&report(), "./doze-aws-data", Style::plain());
    assert!(
        text.starts_with("doze-aws keeps 3.0 GB in ./doze-aws-data\n"),
        "{text}"
    );
    let s3 = text.find("  S3").unwrap();
    let sqs = text.find("  SQS").unwrap();
    assert!(s3 < sqs, "{text}");
    assert!(text.contains("2 queues"), "{text}");
    assert!(text.contains("largest: orders-dlq 86 MB"), "{text}");
    assert!(!text.contains("of it is free space"), "{text}");
    assert!(text.contains("Worth a look"), "{text}");
    assert!(
        text.contains("doze-aws data empty s3 uploads --old-versions"),
        "{text}"
    );
    // The largest note first.
    assert!(
        text.find("uploads (S3)").unwrap() < text.find("orders-dlq (SQS)").unwrap(),
        "{text}"
    );
}

#[test]
fn nothing_kept_says_so() {
    let empty = json!({"data_dir": "/x", "bytes": 0, "services": []});
    assert_eq!(
        overview(&empty, "./d", Style::plain()),
        "doze-aws keeps nothing yet in ./d.\n"
    );
}

#[test]
fn one_service_lists_its_resources_largest_first() {
    let r = report();
    let sqs = &r["services"][0];
    let text = service(sqs, Style::plain());
    assert!(
        text.find("orders-dlq").unwrap() < text.find("jobs").unwrap(),
        "{text}"
    );
    assert!(text.contains("212,418 messages"), "{text}");
    let iam = &r["services"][2];
    assert!(service(iam, Style::plain()).contains("Settings and definitions only"));
}

#[test]
fn one_resource_shows_its_notes_and_the_ways_to_prune_it() {
    let r = report();
    let s3 = &r["services"][1];
    let text = resource(s3, &s3["resources"][0], Style::plain());
    assert!(text.contains("2.0 GB in 41,203 objects"), "{text}");
    assert!(text.contains("! in 3,120 old versions (1.0 GB)"), "{text}");
    assert!(
        text.contains("doze-aws data empty s3 uploads --old-versions"),
        "{text}"
    );
    assert!(
        text.contains("--older-than <age>, --prefix <p>, --old-versions"),
        "{text}"
    );
}

#[test]
fn a_suggested_age_is_rounded_down_and_odd_names_are_quoted() {
    let r = json!({"name": "/aws/lambda/my fn"});
    let p = json!({"before_ms": now_ms() - 7 * 86_400_000 - 5000});
    assert_eq!(
        suggestion("logs", &r, Some(&p)).unwrap(),
        "doze-aws data empty logs '/aws/lambda/my fn' --older-than 7d"
    );
    assert_eq!(suggestion("logs", &r, None), None);
}
