//! Templates: YAML with short forms, every intrinsic, conditions, mappings, parameters, rules and the language extensions.

use serde_json::{Value, json};

use super::World;

const YAML: &str = r"
AWSTemplateFormatVersion: '2010-09-09'
Parameters:
  Env: {Type: String, AllowedValues: [dev, prod], Default: dev}
  Names: {Type: CommaDelimitedList, Default: 'a,b,c'}
  Count: {Type: Number, Default: 2, MinValue: 1}
Mappings:
  Sizes:
    dev: {Memory: 128}
    prod: {Memory: 1024}
Conditions:
  IsProd: !Equals [!Ref Env, prod]
  NotProd: !Not [!Condition IsProd]
  Both: !And [!Condition NotProd, !Equals [!Select [1, !Ref Names], b]]
Resources:
  Q:
    Type: AWS::SQS::Queue
    Properties:
      QueueName: !Sub '${AWS::StackName}-${Env}'
      DelaySeconds: !If [IsProd, 5, !Ref AWS::NoValue]
  OnlyProd:
    Type: AWS::SQS::Queue
    Condition: IsProd
Outputs:
  Name: {Value: !GetAtt Q.QueueName}
  Joined: {Value: !Join ['-', [!Select [0, !Split [',', 'x,y']], !Ref Count]]}
  Mem: {Value: !FindInMap [Sizes, !Ref Env, Memory]}
  Azs: {Value: !Select [0, !GetAZs '']}
  Cidr: {Value: !Select [1, !Cidr ['10.0.0.0/16', 3, 8]]}
  B64: {Value: !Base64 hi}
  Both: {Value: !If [Both, yes-both, no]}
  Literal: {Value: !Sub '${!NotAVar}'}
  Prod:
    Condition: IsProd
    Value: !Ref OnlyProd
";

#[tokio::test]
async fn a_yaml_template_with_every_intrinsic() {
    let w = World::new();
    assert_eq!(
        w.create("y1", Value::String(YAML.into()), &[]).await,
        "CREATE_COMPLETE"
    );
    let o = w.outputs("y1").await;
    assert_eq!(o["Name"], "y1-dev");
    assert_eq!(o["Joined"], "x-2");
    assert_eq!(o["Mem"], "128");
    assert_eq!(o["Azs"], "us-east-1a");
    assert_eq!(o["Cidr"], "10.0.1.0/24");
    assert_eq!(o["B64"], "aGk=");
    assert_eq!(o["Both"], "yes-both");
    assert_eq!(o["Literal"], "${NotAVar}");
    assert!(!o.contains_key("Prod"));
    let res = w
        .cfn("ListStackResources", json!({"StackName": "y1"}))
        .await
        .unwrap();
    assert_eq!(res["StackResourceSummaries"].as_array().unwrap().len(), 1);
    let url = w
        .call("sqs", "GetQueueUrl", json!({"QueueName": "y1-dev"}))
        .await
        .unwrap()["QueueUrl"]
        .clone();
    let attrs = w
        .call(
            "sqs",
            "GetQueueAttributes",
            json!({"QueueUrl": url, "AttributeNames": ["DelaySeconds"]}),
        )
        .await
        .unwrap();
    assert_eq!(attrs["Attributes"]["DelaySeconds"], "0");
}

#[tokio::test]
async fn parameters_are_checked_as_aws_words_it() {
    let w = World::new();
    for (params, why) in [
        (
            json!([{"ParameterKey": "Env", "ParameterValue": "qa"}]),
            "Parameter 'Env' must be one of AllowedValues",
        ),
        (
            json!([{"ParameterKey": "Count", "ParameterValue": "0"}]),
            "Parameter 'Count' must be a number not less than 1",
        ),
        (
            json!([{"ParameterKey": "Nope", "ParameterValue": "0"}]),
            "Parameters: [Nope] do not exist in the template",
        ),
    ] {
        let body = json!({"StackName": "p1", "TemplateBody": YAML, "Parameters": params});
        assert_eq!(w.cfn("CreateStack", body).await.unwrap_err().message, why);
    }
}

#[tokio::test]
async fn ssm_typed_parameters_and_rules() {
    let w = World::new();
    w.call(
        "ssm",
        "PutParameter",
        json!({"Name": "/cfg/size", "Value": "large", "Type": "String"}),
    )
    .await
    .unwrap();
    let t = json!({"Parameters": {"Size": {"Type": "AWS::SSM::Parameter::Value<String>", "Default": "/cfg/size"}},
        "Rules": {"OnlyLarge": {"Assertions": [{"Assert": {"Fn::Contains": [["large", "huge"], {"Ref": "Size"}]}, "AssertDescription": "size must be large"}]}},
        "Resources": {"P": {"Type": "AWS::SSM::Parameter", "Properties": {"Type": "String", "Value": {"Ref": "Size"}}}},
        "Outputs": {"S": {"Value": {"Ref": "Size"}}}});
    assert_eq!(w.create("r1", t.clone(), &[]).await, "CREATE_COMPLETE");
    assert_eq!(w.outputs("r1").await["S"], "large");
    let described = w
        .cfn("DescribeStacks", json!({"StackName": "r1"}))
        .await
        .unwrap();
    assert_eq!(
        described["Stacks"][0]["Parameters"][0]["ResolvedValue"],
        "large"
    );
    w.call(
        "ssm",
        "PutParameter",
        json!({"Name": "/cfg/size", "Value": "small", "Type": "String", "Overwrite": true}),
    )
    .await
    .unwrap();
    let e = w
        .cfn(
            "CreateStack",
            json!({"StackName": "r2", "TemplateBody": t.to_string()}),
        )
        .await
        .unwrap_err();
    assert_eq!(e.message, "size must be large");
    let missing = json!({"Parameters": {"Size": {"Type": "AWS::SSM::Parameter::Value<String>", "Default": "/cfg/none"}},
        "Resources": {"Q": {"Type": "AWS::SQS::Queue"}}});
    let e = w
        .cfn(
            "CreateStack",
            json!({"StackName": "r3", "TemplateBody": missing.to_string()}),
        )
        .await
        .unwrap_err();
    assert!(e.message.contains("/cfg/none"), "{}", e.message);
}

#[tokio::test]
async fn the_language_extensions_expand_loops() {
    let w = World::new();
    let t = json!({"Transform": "AWS::LanguageExtensions",
        "Parameters": {"Names": {"Type": "CommaDelimitedList", "Default": "red,blue"}},
        "Resources": {"Fn::ForEach::Queues": ["N", {"Ref": "Names"}, {
            "Q${N}": {"Type": "AWS::SQS::Queue", "Properties": {"QueueName": {"Fn::Sub": "lx-${N}"}}}}]},
        "Outputs": {"Count": {"Value": {"Fn::Length": {"Ref": "Names"}}}, "Json": {"Value": {"Fn::ToJsonString": {"a": [1]}}}}});
    assert_eq!(w.create("l1", t, &[]).await, "CREATE_COMPLETE");
    assert!(
        w.call("sqs", "GetQueueUrl", json!({"QueueName": "lx-blue"}))
            .await
            .is_ok()
    );
    let o = w.outputs("l1").await;
    assert_eq!(o["Count"], "2");
    assert_eq!(o["Json"], "{\"a\":[1]}");
}

#[tokio::test]
async fn validate_template_and_its_refusals() {
    let w = World::new();
    let ok = w
        .cfn("ValidateTemplate", json!({"TemplateBody": YAML}))
        .await
        .unwrap();
    assert_eq!(ok["Parameters"].as_array().unwrap().len(), 3);
    let cycle = json!({"Resources": {"A": {"Type": "AWS::SQS::Queue", "DependsOn": "B"}, "B": {"Type": "AWS::SQS::Queue", "DependsOn": "A"}}});
    let e = w
        .cfn(
            "CreateStack",
            json!({"StackName": "c1", "TemplateBody": cycle.to_string()}),
        )
        .await
        .unwrap_err();
    assert!(
        e.message
            .starts_with("Circular dependency between resources"),
        "{}",
        e.message
    );
    let e = w
        .cfn("ValidateTemplate", json!({"TemplateBody": "{nope"}))
        .await
        .unwrap_err();
    assert_eq!(
        e.message,
        "Template format error: YAML not well-formed. (line 1, column 6)"
    );
    let iam = json!({"Resources": {"R": {"Type": "AWS::IAM::Role", "Properties": {"RoleName": "x", "AssumeRolePolicyDocument": {}}}}});
    let e = w
        .cfn(
            "CreateStack",
            json!({"StackName": "c2", "TemplateBody": iam.to_string()}),
        )
        .await
        .unwrap_err();
    assert_eq!(e.message, "Requires capabilities : [CAPABILITY_NAMED_IAM]");
}
