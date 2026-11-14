//! Stacks: what they make, what a failure takes back, what an update replaces, and what a delete leaves.

use serde_json::json;

use super::World;

fn queue(name: &str) -> serde_json::Value {
    json!({"Type": "AWS::SQS::Queue", "Properties": {"QueueName": name}})
}

#[tokio::test]
async fn a_stack_makes_its_resources_and_its_delete_takes_them_back() {
    let w = World::new();
    let t = json!({"Resources": {
        "Q": queue("cfn-q"),
        "T": {"Type": "AWS::SNS::Topic", "Properties": {"TopicName": "cfn-t"}},
        "P": {"Type": "AWS::SSM::Parameter", "Properties": {"Name": "/cfn/q", "Type": "String",
            "Value": {"Fn::GetAtt": ["Q", "Arn"]}}},
        "B": {"Type": "AWS::S3::Bucket", "Properties": {"BucketName": "cfn-bucket",
            "VersioningConfiguration": {"Status": "Enabled"}}},
        "D": {"Type": "AWS::DynamoDB::Table", "Properties": {"TableName": "cfn-table", "BillingMode": "PAY_PER_REQUEST",
            "AttributeDefinitions": [{"AttributeName": "id", "AttributeType": "S"}],
            "KeySchema": [{"AttributeName": "id", "KeyType": "HASH"}]}}},
        "Outputs": {"Url": {"Value": {"Ref": "Q"}, "Export": {"Name": "the-url"}}}});
    assert_eq!(w.create("s1", t, &[]).await, "CREATE_COMPLETE");
    let got = w
        .call("ssm", "GetParameter", json!({"Name": "/cfn/q"}))
        .await
        .unwrap();
    assert_eq!(
        got["Parameter"]["Value"],
        "arn:aws:sqs:us-east-1:111122223333:cfn-q"
    );
    let v = w.rest_versioning().await;
    assert_eq!(v, "Enabled");
    assert!(
        w.call(
            "dynamodb",
            "DescribeTable",
            json!({"TableName": "cfn-table"})
        )
        .await
        .is_ok()
    );
    let exports = w.cfn("ListExports", json!({})).await.unwrap();
    assert_eq!(exports["Exports"][0]["Name"], "the-url");
    assert_eq!(
        w.outputs("s1").await["Url"],
        "http://localhost:4566/111122223333/cfn-q"
    );
    assert_eq!(w.delete("s1").await, "DELETE_COMPLETE");
    assert!(
        w.call("sqs", "GetQueueUrl", json!({"QueueName": "cfn-q"}))
            .await
            .is_err()
    );
    assert!(
        w.call("ssm", "GetParameter", json!({"Name": "/cfn/q"}))
            .await
            .is_err()
    );
    assert!(
        w.call(
            "dynamodb",
            "DescribeTable",
            json!({"TableName": "cfn-table"})
        )
        .await
        .is_err()
    );
    let gone = w
        .cfn("DescribeStacks", json!({"StackName": "s1"}))
        .await
        .unwrap_err();
    assert_eq!(gone.message, "Stack with id s1 does not exist");
}

impl World {
    async fn rest_versioning(&self) -> String {
        let r = self
            .peers
            .call_rest(
                "test",
                &self.caller,
                "s3",
                "GetBucketVersioning",
                &json!({"Bucket": "cfn-bucket"}),
            )
            .await
            .unwrap();
        r.json["Status"].as_str().unwrap_or_default().to_owned()
    }
}

#[tokio::test]
async fn a_failed_create_takes_back_what_it_made() {
    let w = World::new();
    let t = json!({"Resources": {"Good": queue("cfn-good"),
        "Bad": {"Type": "AWS::SQS::Queue", "DependsOn": "Good", "Properties": {"VisibilityTimeout": 99999}}}});
    assert_eq!(w.create("s2", t, &[]).await, "ROLLBACK_COMPLETE");
    assert!(
        w.call("sqs", "GetQueueUrl", json!({"QueueName": "cfn-good"}))
            .await
            .is_err()
    );
    let why = w.reason("s2", "Bad", "CREATE_FAILED").await;
    assert!(
        why.starts_with("Resource handler returned message: "),
        "{why}"
    );
    let stack_reason = w.reason("s2", "s2", "ROLLBACK_IN_PROGRESS").await;
    assert_eq!(
        stack_reason,
        "The following resource(s) failed to create: [Bad]. Rollback requested by user."
    );
}

#[tokio::test]
async fn an_update_replaces_or_updates_in_place_and_a_failed_one_goes_back() {
    let w = World::new();
    let v1 = json!({"Resources": {"Q": {"Type": "AWS::SQS::Queue", "Properties": {"VisibilityTimeout": 30}},
        "P": {"Type": "AWS::SSM::Parameter", "Properties": {"Name": "/cfn/u", "Type": "String", "Value": {"Ref": "Q"}}}}});
    assert_eq!(w.create("s3", v1, &[]).await, "CREATE_COMPLETE");
    let first = w
        .call("ssm", "GetParameter", json!({"Name": "/cfn/u"}))
        .await
        .unwrap()["Parameter"]["Value"]
        .clone();
    // FifoQueue is create-only: the queue is replaced, and what refers to it follows.
    let v2 = json!({"Resources": {"Q": {"Type": "AWS::SQS::Queue", "Properties": {"VisibilityTimeout": 30, "FifoQueue": true}},
        "P": {"Type": "AWS::SSM::Parameter", "Properties": {"Name": "/cfn/u", "Type": "String", "Value": {"Ref": "Q"}}}}});
    w.cfn(
        "UpdateStack",
        json!({"StackName": "s3", "TemplateBody": v2.to_string()}),
    )
    .await
    .unwrap();
    assert_eq!(w.settle("s3").await, "UPDATE_COMPLETE");
    let second = w
        .call("ssm", "GetParameter", json!({"Name": "/cfn/u"}))
        .await
        .unwrap()["Parameter"]["Value"]
        .clone();
    assert_ne!(first, second);
    assert!(second.as_str().unwrap().ends_with(".fifo"));
    let old_name = first
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_owned();
    assert!(
        w.call("sqs", "GetQueueUrl", json!({"QueueName": old_name}))
            .await
            .is_err()
    );
    // A failed update goes back to the template before it.
    let v3 = json!({"Resources": {"Q": {"Type": "AWS::SQS::Queue", "Properties": {"VisibilityTimeout": 31, "FifoQueue": true}},
        "P": {"Type": "AWS::SSM::Parameter", "Properties": {"Name": "/cfn/u", "Type": "String", "Value": {"Ref": "Q"}}},
        "Bad": {"Type": "AWS::SSM::Parameter", "DependsOn": "Q", "Properties": {"Name": "/has space", "Type": "String", "Value": "x"}}}});
    w.cfn(
        "UpdateStack",
        json!({"StackName": "s3", "TemplateBody": v3.to_string()}),
    )
    .await
    .unwrap();
    assert_eq!(w.settle("s3").await, "UPDATE_ROLLBACK_COMPLETE");
    let attrs = w
        .call(
            "sqs",
            "GetQueueAttributes",
            json!({"QueueUrl": second, "AttributeNames": ["VisibilityTimeout"]}),
        )
        .await
        .unwrap();
    assert_eq!(attrs["Attributes"]["VisibilityTimeout"], "30");
    let statuses: Vec<String> = w
        .events("s3")
        .await
        .into_iter()
        .filter(|(l, _, _)| l == "s3")
        .map(|(_, s, _)| s)
        .collect();
    assert_eq!(
        statuses.last().map(String::as_str),
        Some("UPDATE_ROLLBACK_COMPLETE")
    );
}

#[tokio::test]
async fn a_custom_named_resource_cannot_be_replaced() {
    let w = World::new();
    assert_eq!(
        w.create("s4", json!({"Resources": {"Q": queue("cfn-named")}}), &[])
            .await,
        "CREATE_COMPLETE"
    );
    let v2 = json!({"Resources": {"Q": {"Type": "AWS::SQS::Queue", "Properties": {"QueueName": "cfn-named", "FifoQueue": true}}}});
    w.cfn(
        "UpdateStack",
        json!({"StackName": "s4", "TemplateBody": v2.to_string()}),
    )
    .await
    .unwrap();
    assert_eq!(w.settle("s4").await, "UPDATE_ROLLBACK_COMPLETE");
    let why = w.reason("s4", "Q", "UPDATE_FAILED").await;
    assert!(
        why.contains("custom-named resource requires replacing"),
        "{why}"
    );
}

#[tokio::test]
async fn roles_and_policies_land_in_iam() {
    let w = World::new();
    let trust = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": {"Service": "lambda.amazonaws.com"}, "Action": "sts:AssumeRole"}]});
    let t = json!({"Resources": {
        "R": {"Type": "AWS::IAM::Role", "Properties": {"RoleName": "cfn-role", "AssumeRolePolicyDocument": trust,
            "Policies": [{"PolicyName": "inline", "PolicyDocument": {"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": "sqs:*", "Resource": "*"}]}}]}},
        "M": {"Type": "AWS::IAM::ManagedPolicy", "Properties": {"Roles": [{"Ref": "R"}],
            "PolicyDocument": {"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": "s3:*", "Resource": "*"}]}}},
        "U": {"Type": "AWS::IAM::User", "Properties": {"UserName": "cfn-user"}},
        "K": {"Type": "AWS::IAM::AccessKey", "Properties": {"UserName": {"Ref": "U"}}}},
        "Outputs": {"Secret": {"Value": {"Fn::GetAtt": ["K", "SecretAccessKey"]}}, "Arn": {"Value": {"Fn::GetAtt": ["R", "Arn"]}}}});
    assert_eq!(w.create("s5", t, &[]).await, "CREATE_COMPLETE");
    let role = w
        .call("iam", "GetRole", json!({"RoleName": "cfn-role"}))
        .await
        .unwrap();
    assert_eq!(
        role["Role"]["Arn"],
        "arn:aws:iam::111122223333:role/cfn-role"
    );
    let attached = w
        .call(
            "iam",
            "ListAttachedRolePolicies",
            json!({"RoleName": "cfn-role"}),
        )
        .await
        .unwrap();
    assert_eq!(attached["AttachedPolicies"].as_array().unwrap().len(), 1);
    assert!(!w.outputs("s5").await["Secret"].as_str().unwrap().is_empty());
    assert_eq!(w.delete("s5").await, "DELETE_COMPLETE");
    assert!(
        w.call("iam", "GetRole", json!({"RoleName": "cfn-role"}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn opaque_types_are_skipped_and_said_so_and_behaviour_bearing_ones_are_refused() {
    let w = World::new();
    let t = json!({"Resources": {"V": {"Type": "AWS::EC2::VPC", "Properties": {"CidrBlock": "10.0.0.0/16"}},
        "S": {"Type": "AWS::EC2::Subnet", "Properties": {"VpcId": {"Ref": "V"}, "CidrBlock": "10.0.1.0/24"}},
        "P": {"Type": "AWS::SSM::Parameter", "Properties": {"Type": "String", "Value": {"Ref": "S"}}}}});
    assert_eq!(w.create("s6", t, &[]).await, "CREATE_COMPLETE");
    let why = w.reason("s6", "V", "CREATE_COMPLETE").await;
    assert!(
        why.starts_with("Not created locally by doze-aws: stored, not active"),
        "{why}"
    );
    let refused = w
        .cfn("CreateStack", json!({"StackName": "s7", "TemplateBody": json!({"Resources": {"Db": {"Type": "AWS::RDS::DBInstance"}}}).to_string()}))
        .await
        .unwrap_err();
    assert!(
        refused.message.contains("Db (AWS::RDS::DBInstance)"),
        "{}",
        refused.message
    );
    let unknown = w
        .cfn("CreateStack", json!({"StackName": "s8", "TemplateBody": json!({"Resources": {"X": {"Type": "AWS::Nope::Thing"}}}).to_string()}))
        .await
        .unwrap_err();
    assert!(
        unknown.message.contains("doze-aws does not serve Nope"),
        "{}",
        unknown.message
    );
}

#[tokio::test]
async fn a_switch_skips_what_is_refused_by_default() {
    let w = World::with(crate::Config {
        unsupported: crate::Unsupported::Skip,
        ..crate::Config::default()
    });
    let t = json!({"Resources": {"Db": {"Type": "AWS::RDS::DBInstance", "Properties": {"DBInstanceClass": "db.t3.micro"}}},
        "Outputs": {"Host": {"Value": {"Fn::GetAtt": ["Db", "Endpoint.Address"]}}}});
    assert_eq!(w.create("s9", t, &[]).await, "CREATE_COMPLETE");
    assert!(
        w.outputs("s9").await["Host"]
            .as_str()
            .unwrap()
            .ends_with(".invalid")
    );
}

#[tokio::test]
async fn dynamic_references_resolve_and_an_unresolvable_one_fails() {
    let w = World::new();
    w.call(
        "ssm",
        "PutParameter",
        json!({"Name": "/src", "Value": "from-ssm", "Type": "String"}),
    )
    .await
    .unwrap();
    w.call(
        "secretsmanager",
        "CreateSecret",
        json!({"Name": "sec", "SecretString": "{\"pw\":\"hunter2\"}"}),
    )
    .await
    .unwrap();
    let t = json!({"Resources": {
        "A": {"Type": "AWS::SSM::Parameter", "Properties": {"Name": "/a", "Type": "String", "Value": "{{resolve:ssm:/src}}"}},
        "B": {"Type": "AWS::SSM::Parameter", "Properties": {"Name": "/b", "Type": "String", "Value": "x-{{resolve:secretsmanager:sec:SecretString:pw}}"}}}});
    assert_eq!(w.create("d1", t, &[]).await, "CREATE_COMPLETE");
    assert_eq!(
        w.call("ssm", "GetParameter", json!({"Name": "/b"}))
            .await
            .unwrap()["Parameter"]["Value"],
        "x-hunter2"
    );
    let bad = json!({"Resources": {"C": {"Type": "AWS::SSM::Parameter", "Properties": {"Name": "/c", "Type": "String", "Value": "{{resolve:ssm:/nope}}"}}}});
    assert_eq!(w.create("d2", bad, &[]).await, "ROLLBACK_COMPLETE");
    assert!(
        w.call("ssm", "GetParameter", json!({"Name": "/c"}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn an_export_in_use_keeps_its_stack() {
    let w = World::new();
    let a = json!({"Resources": {"Q": queue("cfn-ex")}, "Outputs": {"U": {"Value": {"Ref": "Q"}, "Export": {"Name": "shared"}}}});
    assert_eq!(w.create("ea", a, &[]).await, "CREATE_COMPLETE");
    let b = json!({"Resources": {"P": {"Type": "AWS::SSM::Parameter", "Properties": {"Type": "String", "Value": {"Fn::ImportValue": "shared"}}}}});
    assert_eq!(w.create("eb", b, &[]).await, "CREATE_COMPLETE");
    let imports = w
        .cfn("ListImports", json!({"ExportName": "shared"}))
        .await
        .unwrap();
    assert_eq!(imports["Imports"], json!(["eb"]));
    assert_eq!(w.delete("ea").await, "DELETE_FAILED");
    assert_eq!(w.delete("eb").await, "DELETE_COMPLETE");
}
