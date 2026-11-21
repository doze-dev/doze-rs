//! CloudFormation's stack and export pages: the operations the wizards and a change set's page make as the console, against a
//! real CloudFormation, and no others; the deleted stacks read by their id; the exports and who imports them.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use std::time::Duration;

use aws_peers::Mode;
use serde_json::{Value, json};
use world::world;

const BASE: &str = "/api/v1/resources/cloudformation";

fn template(queue: &str) -> String {
    json!({"Parameters": {"Stage": {"Type": "String", "Default": "dev"}},
        "Resources": {"Inbox": {"Type": "AWS::SQS::Queue", "Properties": {"QueueName": queue}}},
        "Outputs": {"InboxArn": {"Value": {"Fn::GetAtt": ["Inbox", "Arn"]}, "Export": {"Name": format!("{queue}-arn")}}}})
    .to_string()
}

async fn op(w: &world::World, kind: &str, operation: &str, input: &Value) -> (u16, Value) {
    w.send("POST", &format!("{BASE}/{kind}/op/{operation}"), input)
        .await
}

async fn stack_status(w: &world::World, name: &str) -> String {
    for _ in 0..100 {
        let (_, d) = op(w, "stacks", "DescribeStacks", &json!({"StackName": name})).await;
        let status = d["Stacks"][0]["StackStatus"].as_str().unwrap_or("");
        if !status.ends_with("_IN_PROGRESS") && !status.is_empty() {
            return status.to_owned();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{name} did not settle");
}

#[tokio::test]
async fn a_template_is_validated_summarised_and_made_into_a_stack_by_the_wizards_calls() {
    let w = world("cfn-wizard", Mode::Soft, &[]);
    let body = template("wiz-inbox");
    // Step 1: ValidateTemplate, then step 2's GetTemplateSummary, which lists the parameters.
    let (status, valid) = op(
        &w,
        "stacks",
        "ValidateTemplate",
        &json!({"TemplateBody": body}),
    )
    .await;
    assert_eq!(status, 200, "{valid}");
    let (status, summary) = op(
        &w,
        "stacks",
        "GetTemplateSummary",
        &json!({"TemplateBody": body}),
    )
    .await;
    assert_eq!(status, 200, "{summary}");
    assert_eq!(summary["Parameters"][0]["ParameterKey"], "Stage");
    assert_eq!(summary["ResourceTypes"][0], "AWS::SQS::Queue");
    // A template that is not one is refused with the service's own code.
    let (status, bad) = op(
        &w,
        "stacks",
        "ValidateTemplate",
        &json!({"TemplateBody": "{ not"}),
    )
    .await;
    assert_eq!(
        (status, bad["code"].as_str()),
        (400, Some("ValidationError")),
        "{bad}"
    );
    // Step 4: CreateStack keeps the stack policy and the termination protection the options asked for, and the tags.
    let (status, made) = op(
        &w,
        "stacks",
        "CreateStack",
        &json!({"StackName": "wiz", "TemplateBody": body, "EnableTerminationProtection": true,
            "StackPolicyBody": r#"{"Statement":[{"Effect":"Allow","Action":"Update:*","Principal":"*","Resource":"*"}]}"#,
            "Tags": [{"Key": "app", "Value": "larder"}], "NotificationARNs": []}),
    )
    .await;
    assert_eq!(status, 200, "{made}");
    assert!(made["StackId"].as_str().unwrap().contains(":stack/wiz/"));
    assert_eq!(stack_status(&w, "wiz").await, "CREATE_COMPLETE");
    let (_, d) = op(&w, "stacks", "DescribeStacks", &json!({"StackName": "wiz"})).await;
    assert_eq!(d["Stacks"][0]["EnableTerminationProtection"], true, "{d}");
    assert_eq!(d["Stacks"][0]["Tags"][0]["Value"], "larder");
    let (_, policy) = op(&w, "stacks", "GetStackPolicy", &json!({"StackName": "wiz"})).await;
    assert!(
        policy["StackPolicyBody"]
            .as_str()
            .unwrap()
            .contains("Update:*")
    );
    // Termination protection is changed from the stack's page, and DeleteStack is then refused.
    let (status, set) = op(
        &w,
        "stacks",
        "UpdateTerminationProtection",
        &json!({"StackName": "wiz", "EnableTerminationProtection": false}),
    )
    .await;
    assert_eq!(status, 200, "{set}");
    let (_, d) = op(&w, "stacks", "DescribeStacks", &json!({"StackName": "wiz"})).await;
    assert_eq!(d["Stacks"][0]["EnableTerminationProtection"], false);
    let (_, on) = op(
        &w,
        "stacks",
        "UpdateTerminationProtection",
        &json!({"StackName": "wiz", "EnableTerminationProtection": true}),
    )
    .await;
    assert!(on["StackId"].is_string(), "{on}");
    // A name that is taken is refused with AlreadyExistsException.
    let (status, again) = op(
        &w,
        "stacks",
        "CreateStack",
        &json!({"StackName": "wiz", "TemplateBody": body}),
    )
    .await;
    assert_eq!(
        (status, again["code"].as_str()),
        (400, Some("AlreadyExistsException")),
        "{again}"
    );
    // Update step 1 keeps the template (UsePreviousTemplate) and step 2 sends the parameters as they are.
    let (status, same) = op(
        &w,
        "stacks",
        "UpdateStack",
        &json!({"StackName": "wiz", "UsePreviousTemplate": true, "Parameters": [{"ParameterKey": "Stage", "UsePreviousValue": true}]}),
    )
    .await;
    assert_eq!(status, 400, "{same}");
    assert!(
        same["message"].as_str().unwrap().contains("No updates"),
        "{same}"
    );
    // The calls are the console's own, in Live.
    assert!(
        w.live()
            .iter()
            .any(|l| l == "console cloudformation CreateStack"),
        "{:?}",
        w.live()
    );
    // An operation the kind does not list is not made.
    let (status, refused) = op(&w, "stacks", "DeleteStack", &json!({"StackName": "wiz"})).await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (404, Some("NOT_IN_PAGE"))
    );
}

#[tokio::test]
async fn a_change_set_is_made_read_executed_and_deleted_from_its_page() {
    let w = world("cfn-changeset", Mode::Soft, &[]);
    let (status, made) = op(
        &w,
        "stacks",
        "CreateChangeSet",
        &json!({"StackName": "review", "ChangeSetName": "initial", "ChangeSetType": "CREATE", "TemplateBody": template("cs-inbox")}),
    )
    .await;
    assert_eq!(status, 200, "{made}");
    // The stack is made at once, in review, with the change set that would build it.
    let (_, d) = op(
        &w,
        "stacks",
        "DescribeStacks",
        &json!({"StackName": "review"}),
    )
    .await;
    assert_eq!(d["Stacks"][0]["StackStatus"], "REVIEW_IN_PROGRESS", "{d}");
    let (status, set) = op(
        &w,
        "stacks",
        "DescribeChangeSet",
        &json!({"StackName": "review", "ChangeSetName": "initial"}),
    )
    .await;
    assert_eq!(status, 200, "{set}");
    assert_eq!(set["ExecutionStatus"], "AVAILABLE");
    assert_eq!(set["Changes"][0]["ResourceChange"]["Action"], "Add");
    let (_, listed) = op(
        &w,
        "stacks",
        "ListChangeSets",
        &json!({"StackName": "review"}),
    )
    .await;
    assert_eq!(listed["Summaries"][0]["ChangeSetName"], "initial");
    // Executing builds the stack, and the set then reads EXECUTE_COMPLETE.
    let (status, run) = op(
        &w,
        "stacks",
        "ExecuteChangeSet",
        &json!({"StackName": "review", "ChangeSetName": "initial"}),
    )
    .await;
    assert_eq!(status, 200, "{run}");
    assert_eq!(stack_status(&w, "review").await, "CREATE_COMPLETE");
    // A change set with nothing in it fails, and cannot be executed: the service's own code.
    let (_, empty) = op(
        &w,
        "stacks",
        "CreateChangeSet",
        &json!({"StackName": "review", "ChangeSetName": "same", "TemplateBody": template("cs-inbox")}),
    )
    .await;
    assert!(empty["Id"].is_string(), "{empty}");
    let (_, set) = op(
        &w,
        "stacks",
        "DescribeChangeSet",
        &json!({"StackName": "review", "ChangeSetName": "same"}),
    )
    .await;
    assert_eq!(
        (set["Status"].as_str(), set["ExecutionStatus"].as_str()),
        (Some("FAILED"), Some("UNAVAILABLE"))
    );
    let (status, refused) = op(
        &w,
        "stacks",
        "ExecuteChangeSet",
        &json!({"StackName": "review", "ChangeSetName": "same"}),
    )
    .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (400, Some("InvalidChangeSetStatus")),
        "{refused}"
    );
    let (status, gone) = op(
        &w,
        "stacks",
        "DeleteChangeSet",
        &json!({"StackName": "review", "ChangeSetName": "same"}),
    )
    .await;
    assert_eq!(status, 200, "{gone}");
}

#[tokio::test]
async fn a_deleted_stack_is_read_by_its_id_and_the_exports_name_who_imports_them() {
    let w = world("cfn-exports", Mode::Soft, &[]);
    let (_, none) = w.get(&format!("{BASE}/exports")).await;
    assert!(none["rows"].as_array().unwrap().is_empty(), "{none}");
    op(
        &w,
        "stacks",
        "CreateStack",
        &json!({"StackName": "base", "TemplateBody": template("exp-inbox")}),
    )
    .await;
    assert_eq!(stack_status(&w, "base").await, "CREATE_COMPLETE");
    // The export is a row, with its value and the stack that makes it.
    let (_, list) = w.get(&format!("{BASE}/exports")).await;
    assert_eq!(list["rows"][0]["name"], "exp-inbox-arn", "{list}");
    assert!(
        list["rows"][0]["cells"][0]
            .as_str()
            .unwrap()
            .contains(":exp-inbox")
    );
    assert!(
        list["rows"][0]["cells"][1]
            .as_str()
            .unwrap()
            .contains(":stack/base/")
    );
    assert!(list["openable"].as_bool().unwrap());
    // Nothing imports it: ListImports says so as CloudFormation does, a ValidationError.
    let (status, nobody) = op(
        &w,
        "stacks",
        "ListImports",
        &json!({"ExportName": "exp-inbox-arn"}),
    )
    .await;
    assert_eq!(
        (status, nobody["code"].as_str()),
        (400, Some("ValidationError")),
        "{nobody}"
    );
    // A stack that imports it.
    let user = json!({"Resources": {"Q": {"Type": "AWS::SQS::Queue", "Properties": {"QueueName": "exp-user", "Tags": [{"Key": "from", "Value": {"Fn::ImportValue": "exp-inbox-arn"}}]}}}});
    let (status, made) = op(
        &w,
        "stacks",
        "CreateStack",
        &json!({"StackName": "user", "TemplateBody": user.to_string()}),
    )
    .await;
    assert_eq!(status, 200, "{made}");
    assert_eq!(stack_status(&w, "user").await, "CREATE_COMPLETE");
    let (_, who) = op(
        &w,
        "stacks",
        "ListImports",
        &json!({"ExportName": "exp-inbox-arn"}),
    )
    .await;
    assert_eq!(who["Imports"][0], "user", "{who}");
    // Deleting the exporter while the import stands is not carried out.
    w.send("DELETE", &format!("{BASE}/stacks/base"), &json!({}))
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(stack_status(&w, "base").await, "CREATE_COMPLETE");
    // Deleted stacks: only those, with the id a deleted stack is read by.
    w.send("DELETE", &format!("{BASE}/stacks/user"), &json!({}))
        .await;
    for _ in 0..100 {
        let (_, l) = op(&w, "stacks", "ListStacks", &json!({})).await;
        if !l["StackSummaries"].as_array().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (_, deleted) = op(
        &w,
        "stacks",
        "ListStacks",
        &json!({"StackStatusFilter": ["CREATE_COMPLETE"]}),
    )
    .await;
    let rows = deleted["StackSummaries"].as_array().unwrap();
    assert!(
        rows.iter().all(|s| s["StackStatus"] == "DELETE_COMPLETE"),
        "the filter is the console's: {deleted}"
    );
    let id = rows[0]["StackId"].as_str().unwrap();
    let (status, read) = op(&w, "stacks", "DescribeStacks", &json!({"StackName": id})).await;
    assert_eq!(status, 200, "{read}");
    assert_eq!(read["Stacks"][0]["StackStatus"], "DELETE_COMPLETE");
    let (status, events) = op(
        &w,
        "stacks",
        "DescribeStackEvents",
        &json!({"StackName": id}),
    )
    .await;
    assert_eq!(status, 200, "{events}");
    assert_eq!(
        events["StackEvents"][0]["ResourceStatus"],
        "DELETE_COMPLETE"
    );
}
