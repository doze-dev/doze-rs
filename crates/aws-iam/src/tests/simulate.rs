//! The simulator, through the model like a client would call it.

use serde_json::json;

use super::World;

const ALLOW_SEND: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"sqs:Send*","Resource":"*","Condition":{"StringEquals":{"aws:username":"alice"}}}]}"#;

#[test]
fn a_custom_policy_is_decided_with_the_context_given() {
    let mut w = World::new();
    let ask = |w: &mut World, context: serde_json::Value| {
        w.ok(
            "SimulateCustomPolicy",
            &json!({
                "PolicyInputList": [ALLOW_SEND],
                "ActionNames": ["sqs:SendMessage", "sqs:ReceiveMessage"],
                "ContextEntries": context,
            }),
        )
    };
    let out = ask(
        &mut w,
        json!([{"ContextKeyName": "aws:username", "ContextKeyValues": ["alice"], "ContextKeyType": "string"}]),
    );
    let r = out["EvaluationResults"].as_array().unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(
        (
            r[0]["EvalActionName"].as_str(),
            r[0]["EvalDecision"].as_str()
        ),
        (Some("sqs:SendMessage"), Some("ALLOWED"))
    );
    assert_eq!(
        r[0]["MatchedStatements"][0]["SourcePolicyId"],
        "PolicyInputList.1"
    );
    assert_eq!(r[1]["EvalDecision"], "IMPLICIT_DENY");
    // Without the key the condition cannot hold, and the answer says which key was missing.
    let out = ask(&mut w, json!([]));
    let r = &out["EvaluationResults"][0];
    assert_eq!(r["EvalDecision"], "IMPLICIT_DENY");
    assert_eq!(r["MissingContextValues"], json!(["aws:username"]));
}

#[test]
fn a_principal_is_simulated_with_its_groups_and_its_boundary() {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "alice"}));
    w.ok("CreateGroup", &json!({"GroupName": "ops"}));
    w.ok(
        "AddUserToGroup",
        &json!({"UserName": "alice", "GroupName": "ops"}),
    );
    w.ok("PutGroupPolicy", &json!({"GroupName": "ops", "PolicyName": "queues",
        "PolicyDocument": r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"sqs:*","Resource":"*"}]}"#}));
    let arn = "arn:aws:iam::000000000000:user/alice";
    let decide = |w: &mut World| {
        let out = w.ok(
            "SimulatePrincipalPolicy",
            &json!({"PolicySourceArn": arn, "ActionNames": ["sqs:DeleteQueue"]}),
        );
        out["EvaluationResults"][0].clone()
    };
    let r = decide(&mut w);
    assert_eq!(r["EvalDecision"], "ALLOWED");
    assert_eq!(r["MatchedStatements"][0]["SourcePolicyType"], "group");
    // A boundary that grants only S3 takes the permission away.
    let boundary = w.ok("CreatePolicy", &json!({"PolicyName": "s3only",
        "PolicyDocument": r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"*"}]}"#}));
    w.ok(
        "PutUserPermissionsBoundary",
        &json!({"UserName": "alice", "PermissionsBoundary": boundary["Policy"]["Arn"]}),
    );
    let r = decide(&mut w);
    assert_eq!(r["EvalDecision"], "IMPLICIT_DENY");
    assert_eq!(
        r["PermissionsBoundaryDecisionDetail"]["AllowedByPermissionsBoundary"],
        false
    );
    assert_eq!(w.run("SimulatePrincipalPolicy", &json!({"PolicySourceArn": "arn:aws:iam::000000000000:user/ghost", "ActionNames": ["x:y"]})).unwrap_err().code, "NoSuchEntityException");
}

#[test]
fn context_keys_are_listed_from_policies() {
    let mut w = World::new();
    let out = w.ok(
        "GetContextKeysForCustomPolicy",
        &json!({"PolicyInputList": [ALLOW_SEND]}),
    );
    assert_eq!(out["ContextKeyNames"], json!(["aws:username"]));
    assert_eq!(
        w.run(
            "GetContextKeysForCustomPolicy",
            &json!({"PolicyInputList": ["{not json"]})
        )
        .unwrap_err()
        .code,
        "InvalidInputException"
    );
}
