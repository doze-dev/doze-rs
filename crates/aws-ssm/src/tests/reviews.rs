//! The review of a change template: each action kept, and read back by `ListDocumentMetadataHistory`.

use serde_json::{Value, json};

use super::World;

fn content(n: u32) -> String {
    json!({"schemaVersion": "0.3", "description": format!("v{n}"),
        "mainSteps": [{"name": "s", "action": "aws:sleep", "inputs": {"Duration": "PT1S"}}]})
    .to_string()
}

fn template(w: &mut World) {
    w.ok(
        "CreateDocument",
        &json!({"Name": "Change1", "DocumentType": "Automation.ChangeTemplate", "Content": content(1)}),
    );
    w.ok(
        "UpdateDocument",
        &json!({"Name": "Change1", "DocumentVersion": "$LATEST", "Content": content(2)}),
    );
}

fn review(
    w: &mut World,
    version: &str,
    action: &str,
    comment: Option<&str>,
) -> Result<Value, String> {
    let mut reviews = json!({"Action": action});
    if let Some(c) = comment {
        reviews["Comment"] = json!([{"Type": "Comment", "Content": c}]);
    }
    w.run(
        "UpdateDocumentMetadata",
        &json!({"Name": "Change1", "DocumentVersion": version, "DocumentReviews": reviews}),
    )
    .map_err(|e| e.code.to_string())
}

fn history(w: &mut World, version: &str) -> Value {
    w.ok(
        "ListDocumentMetadataHistory",
        &json!({"Name": "Change1", "DocumentVersion": version, "Metadata": "DocumentReviews"}),
    )
}

fn status(w: &mut World, version: &str) -> Value {
    w.ok(
        "DescribeDocument",
        &json!({"Name": "Change1", "DocumentVersion": version}),
    )["Document"]["ReviewStatus"]
        .clone()
}

#[test]
fn each_review_action_is_kept_and_listed() {
    let mut w = World::new();
    template(&mut w);
    let empty = history(&mut w, "1");
    assert_eq!(empty["Name"], "Change1");
    assert_eq!(empty["DocumentVersion"], "1");
    assert_eq!(empty["Metadata"]["ReviewerResponse"], json!([]));
    review(&mut w, "1", "SendForReview", Some("please")).unwrap();
    w.clock.advance(1_000);
    review(&mut w, "1", "UpdateReview", Some("please, today")).unwrap();
    review(&mut w, "1", "Approve", Some("ok")).unwrap();
    let h = history(&mut w, "1");
    assert_eq!(h["Author"], "000000000000");
    let responses = h["Metadata"]["ReviewerResponse"].as_array().unwrap();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["ReviewStatus"], "PENDING");
    assert_eq!(responses[0]["Comment"][0]["Content"], "please, today");
    assert!(responses[0]["UpdatedTime"].as_f64() > responses[0]["CreateTime"].as_f64());
    assert_eq!(responses[1]["ReviewStatus"], "APPROVED");
    assert_eq!(responses[1]["Comment"][0]["Type"], "Comment");
    assert_eq!(responses[1]["Reviewer"], "000000000000");
    assert_eq!(status(&mut w, "1"), "APPROVED");
    // Paged.
    let first = w.ok(
        "ListDocumentMetadataHistory",
        &json!({"Name": "Change1", "DocumentVersion": "1",
        "Metadata": "DocumentReviews", "MaxResults": 1}),
    );
    assert_eq!(
        first["Metadata"]["ReviewerResponse"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let next = w.ok(
        "ListDocumentMetadataHistory",
        &json!({"Name": "Change1", "DocumentVersion": "1",
        "Metadata": "DocumentReviews", "NextToken": first["NextToken"]}),
    );
    assert_eq!(
        next["Metadata"]["ReviewerResponse"][0]["ReviewStatus"],
        "APPROVED"
    );
    assert!(next.get("NextToken").is_none());
}

#[test]
fn one_version_is_approved_and_one_is_in_review_at_a_time() {
    let mut w = World::new();
    template(&mut w);
    review(&mut w, "1", "Approve", None).unwrap();
    review(&mut w, "2", "SendForReview", None).unwrap();
    // A second version in review at once is refused.
    assert_eq!(
        review(&mut w, "1", "SendForReview", None).unwrap_err(),
        "InvalidDocumentOperation"
    );
    review(&mut w, "2", "Approve", None).unwrap();
    assert_eq!(status(&mut w, "2"), "APPROVED");
    // Approving version 2 rejects the version approved before it.
    assert_eq!(status(&mut w, "1"), "REJECTED");
}

#[test]
fn the_history_refuses_what_is_not_there() {
    let mut w = World::new();
    template(&mut w);
    assert_eq!(
        w.err(
            "ListDocumentMetadataHistory",
            &json!({"Name": "Nope1", "Metadata": "DocumentReviews"})
        )
        .0,
        "InvalidDocument"
    );
    assert_eq!(
        w.err(
            "ListDocumentMetadataHistory",
            &json!({"Name": "Change1", "DocumentVersion": "9", "Metadata": "DocumentReviews"})
        )
        .0,
        "InvalidDocumentVersion"
    );
    assert_eq!(
        review(&mut w, "1", "UpdateReview", None).unwrap_err(),
        "InvalidDocumentOperation"
    );
    // A deleted template takes its reviews with it.
    review(&mut w, "1", "SendForReview", None).unwrap();
    w.ok("DeleteDocument", &json!({"Name": "Change1"}));
    template(&mut w);
    assert_eq!(
        history(&mut w, "1")["Metadata"]["ReviewerResponse"],
        json!([])
    );
}
