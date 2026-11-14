//! Change sets: their records, their execution, and the empty one the AWS CLI looks for.

use serde_json::json;

use super::World;

fn template(visibility: u32, extra: bool) -> String {
    let mut res = json!({
        "Q": {"Type": "AWS::SQS::Queue", "Properties": {"QueueName": "cs-q", "VisibilityTimeout": {"Ref": "V"}}},
        "P": {"Type": "AWS::SSM::Parameter", "Properties": {"Name": "/cs", "Type": "String", "Value": {"Fn::GetAtt": ["Q", "Arn"]}}}});
    if extra {
        res["T"] = json!({"Type": "AWS::SNS::Topic"});
    }
    json!({"Parameters": {"V": {"Type": "Number", "Default": visibility}}, "Resources": res})
        .to_string()
}

#[tokio::test]
async fn a_change_set_says_what_it_would_do_and_does_it() {
    let w = World::new();
    let made = w
        .cfn("CreateChangeSet", json!({"StackName": "c1", "ChangeSetName": "first", "ChangeSetType": "CREATE", "TemplateBody": template(30, false)}))
        .await
        .unwrap();
    let id = made["Id"].as_str().unwrap().to_owned();
    let review = w
        .cfn("DescribeStacks", json!({"StackName": "c1"}))
        .await
        .unwrap();
    assert_eq!(review["Stacks"][0]["StackStatus"], "REVIEW_IN_PROGRESS");
    let d = w
        .cfn("DescribeChangeSet", json!({"ChangeSetName": id}))
        .await
        .unwrap();
    assert_eq!(d["Status"], "CREATE_COMPLETE");
    assert_eq!(d["Changes"].as_array().unwrap().len(), 2);
    assert_eq!(d["Changes"][0]["ResourceChange"]["Action"], "Add");
    w.cfn("ExecuteChangeSet", json!({"ChangeSetName": id}))
        .await
        .unwrap();
    assert_eq!(w.settle("c1").await, "CREATE_COMPLETE");
    let again = w
        .cfn("ExecuteChangeSet", json!({"ChangeSetName": id}))
        .await
        .unwrap_err();
    assert!(
        again.message.contains("EXECUTE_COMPLETE"),
        "{}",
        again.message
    );

    let second = w
        .cfn(
            "CreateChangeSet",
            json!({"StackName": "c1", "ChangeSetName": "second", "TemplateBody": template(30, true),
            "Parameters": [{"ParameterKey": "V", "ParameterValue": "45"}]}),
        )
        .await
        .unwrap();
    let d = w
        .cfn("DescribeChangeSet", json!({"ChangeSetName": second["Id"]}))
        .await
        .unwrap();
    let changes = d["Changes"].as_array().unwrap();
    let q = changes
        .iter()
        .find(|c| c["ResourceChange"]["LogicalResourceId"] == "Q")
        .unwrap();
    assert_eq!(q["ResourceChange"]["Action"], "Modify");
    assert_eq!(q["ResourceChange"]["Replacement"], "False");
    assert_eq!(
        q["ResourceChange"]["Details"][0]["ChangeSource"],
        "ParameterReference"
    );
    assert_eq!(q["ResourceChange"]["Details"][0]["CausingEntity"], "V");
    let t = changes
        .iter()
        .find(|c| c["ResourceChange"]["LogicalResourceId"] == "T")
        .unwrap();
    assert_eq!(t["ResourceChange"]["Action"], "Add");
    assert!(
        changes
            .iter()
            .all(|c| c["ResourceChange"]["LogicalResourceId"] != "P")
    );
    w.cfn("ExecuteChangeSet", json!({"ChangeSetName": second["Id"]}))
        .await
        .unwrap();
    assert_eq!(w.settle("c1").await, "UPDATE_COMPLETE");

    let empty = w
        .cfn(
            "CreateChangeSet",
            json!({"StackName": "c1", "ChangeSetName": "third", "UsePreviousTemplate": true,
            "Parameters": [{"ParameterKey": "V", "UsePreviousValue": true}]}),
        )
        .await
        .unwrap();
    let d = w
        .cfn("DescribeChangeSet", json!({"ChangeSetName": empty["Id"]}))
        .await
        .unwrap();
    assert_eq!(d["Status"], "FAILED");
    assert_eq!(
        d["StatusReason"],
        "The submitted information didn't contain changes. Submit different information to create a change set."
    );
    let listed = w
        .cfn("ListChangeSets", json!({"StackName": "c1"}))
        .await
        .unwrap();
    assert_eq!(listed["Summaries"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn a_replacement_is_said_to_be_one() {
    let w = World::new();
    let v1 = json!({"Resources": {"Q": {"Type": "AWS::SQS::Queue"}, "P": {"Type": "AWS::SSM::Parameter",
        "Properties": {"Type": "String", "Value": {"Ref": "Q"}}}}});
    assert_eq!(w.create("c2", v1, &[]).await, "CREATE_COMPLETE");
    let v2 = json!({"Resources": {"Q": {"Type": "AWS::SQS::Queue", "Properties": {"FifoQueue": true}}, "P": {"Type": "AWS::SSM::Parameter",
        "Properties": {"Type": "String", "Value": {"Ref": "Q"}}}}});
    let made = w
        .cfn(
            "CreateChangeSet",
            json!({"StackName": "c2", "ChangeSetName": "r", "TemplateBody": v2.to_string()}),
        )
        .await
        .unwrap();
    let d = w
        .cfn("DescribeChangeSet", json!({"ChangeSetName": made["Id"]}))
        .await
        .unwrap();
    let changes = d["Changes"].as_array().unwrap();
    let q = changes
        .iter()
        .find(|c| c["ResourceChange"]["LogicalResourceId"] == "Q")
        .unwrap();
    assert_eq!(q["ResourceChange"]["Replacement"], "True");
    let p = changes
        .iter()
        .find(|c| c["ResourceChange"]["LogicalResourceId"] == "P")
        .unwrap();
    assert_eq!(p["ResourceChange"]["Details"][0]["Evaluation"], "Dynamic");
}
