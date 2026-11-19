//! Resource policies on parameters and documents: kept with their id and hash, held to the model's rules, and handed to the
//! authorizer as the resource-based layer of every request on the resource.

use aws_iam_policy::{Context, Policies, Request, Subject};
use aws_peers::Need;
use serde_json::{Value, json};

use super::World;

const ARN: &str = "arn:aws:ssm:us-east-1:000000000000:parameter/shared/db";
const USER: &str = "arn:aws:iam::000000000000:user/reader";

fn allow(principal: &str, action: &str) -> String {
    json!({"Version": "2012-10-17", "Statement": [{"Sid": "s", "Effect": "Allow", "Principal": {"AWS": principal},
        "Action": action, "Resource": ARN}]})
    .to_string()
}

fn advanced(w: &mut World) {
    w.ok(
        "PutParameter",
        &json!({"Name": "/shared/db", "Value": "v", "Type": "String", "Tier": "Advanced", "Overwrite": true}),
    );
}

fn put(w: &mut World, extra: &Value) -> Result<Value, (String, String)> {
    let mut r = json!({"ResourceArn": ARN, "Policy": allow(USER, "ssm:GetParameter")});
    for (k, v) in extra.as_object().unwrap() {
        r[k] = v.clone();
    }
    w.run("PutResourcePolicy", &r).map_err(|e| {
        (
            e.shape
                .map_or_else(|| e.code.to_string(), |s| s.to_string()),
            e.message,
        )
    })
}

fn asked(w: &World, op: &str, request: &Value) -> Vec<Need> {
    let ix = w.h.op(op);
    let call = w.h.call(ix, request).unwrap();
    w.core.needs(&call)
}

#[test]
fn a_policy_is_put_read_changed_by_its_hash_and_deleted() {
    let mut w = World::new();
    advanced(&mut w);
    let made = put(&mut w, &json!({})).unwrap();
    let (id, hash) = (
        made["PolicyId"].as_str().unwrap().to_owned(),
        made["PolicyHash"].as_str().unwrap().to_owned(),
    );
    let got = w.ok("GetResourcePolicies", &json!({"ResourceArn": ARN}));
    assert_eq!(got["Policies"][0]["PolicyId"], id.as_str());
    assert_eq!(got["Policies"][0]["PolicyHash"], hash.as_str());
    assert_eq!(
        got["Policies"][0]["Policy"],
        allow(USER, "ssm:GetParameter")
    );
    // A change names the policy and the hash it has now; the answer is a new hash for the same id.
    let changed = put(
        &mut w,
        &json!({"PolicyId": id, "PolicyHash": hash, "Policy": allow(USER, "ssm:GetParameters")}),
    )
    .unwrap();
    assert_eq!(changed["PolicyId"], id.as_str());
    assert_ne!(changed["PolicyHash"], hash.as_str());
    // The old hash is out of date.
    let stale = json!({"PolicyId": id, "PolicyHash": hash});
    assert_eq!(
        put(&mut w, &stale).unwrap_err().0,
        "ResourcePolicyConflictException"
    );
    assert_eq!(
        w.err(
            "DeleteResourcePolicy",
            &json!({"ResourceArn": ARN, "PolicyId": id, "PolicyHash": hash})
        )
        .0,
        "ResourcePolicyConflictException"
    );
    assert_eq!(
        w.err(
            "DeleteResourcePolicy",
            &json!({"ResourceArn": ARN, "PolicyId": "nope", "PolicyHash": hash})
        )
        .0,
        "ResourcePolicyNotFoundException"
    );
    w.ok(
        "DeleteResourcePolicy",
        &json!({"ResourceArn": ARN, "PolicyId": id, "PolicyHash": changed["PolicyHash"]}),
    );
    assert_eq!(
        w.ok("GetResourcePolicies", &json!({"ResourceArn": ARN}))["Policies"],
        json!([])
    );
}

#[test]
fn the_model_s_rules_for_a_policy_are_kept() {
    let mut w = World::new();
    assert_eq!(
        put(&mut w, &json!({})).unwrap_err().0,
        "ResourceNotFoundException"
    );
    // A standard parameter cannot be shared.
    w.put("/shared/db", "v");
    let e = put(&mut w, &json!({})).unwrap_err();
    assert_eq!(e.0, "ResourcePolicyInvalidParameterException");
    assert!(e.1.contains("advanced"));
    advanced(&mut w);
    // Under the AWS managed key a SecureString cannot be shared.
    w.ok(
        "PutParameter",
        &json!({"Name": "/shared/db", "Value": "v", "Type": "SecureString", "Overwrite": true}),
    );
    assert_eq!(
        put(&mut w, &json!({})).unwrap_err().0,
        "ResourcePolicyInvalidParameterException"
    );
    w.ok(
        "PutParameter",
        &json!({"Name": "/shared/db", "Value": "v", "Type": "String", "Overwrite": true}),
    );
    assert_eq!(
        put(&mut w, &json!({"Policy": "{not json"})).unwrap_err().0,
        "MalformedResourcePolicyDocumentException"
    );
    let big = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": "*", "Action": "ssm:*",
        "Resource": "x".repeat(1100)}]})
    .to_string();
    assert_eq!(
        put(&mut w, &json!({"Policy": big})).unwrap_err().0,
        "ResourcePolicyLimitExceededException"
    );
    assert_eq!(
        put(&mut w, &json!({"PolicyId": "x"})).unwrap_err().0,
        "ResourcePolicyInvalidParameterException"
    );
    assert_eq!(
        put(&mut w, &json!({"PolicyId": "x", "PolicyHash": "y"}))
            .unwrap_err()
            .0,
        "ResourcePolicyNotFoundException"
    );
    let other = w.run(
        "PutResourcePolicy",
        &json!({"ResourceArn": "arn:aws:ssm:us-east-1:000000000000:opsitemgroup/default", "Policy": allow(USER, "ssm:*")}),
    );
    assert_eq!(
        other.unwrap_err().code,
        "ResourcePolicyInvalidParameterException"
    );
}

#[test]
fn a_deleted_parameter_takes_its_policies_with_it() {
    let mut w = World::new();
    advanced(&mut w);
    put(&mut w, &json!({})).unwrap();
    w.ok("DeleteParameter", &json!({"Name": "/shared/db"}));
    advanced(&mut w);
    assert_eq!(
        w.ok("GetResourcePolicies", &json!({"ResourceArn": ARN}))["Policies"],
        json!([])
    );
}

#[test]
fn a_document_takes_a_policy_too() {
    let mut w = World::new();
    w.ok(
        "CreateDocument",
        &json!({"Name": "Doc1", "DocumentType": "Command",
        "Content": json!({"schemaVersion": "2.2", "mainSteps": []}).to_string()}),
    );
    let arn = "arn:aws:ssm:us-east-1:000000000000:document/Doc1";
    let policy = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": {"AWS": "111122223333"},
        "Action": "ssm:GetDocument", "Resource": arn}]})
    .to_string();
    w.ok(
        "PutResourcePolicy",
        &json!({"ResourceArn": arn, "Policy": policy}),
    );
    let needs = asked(&w, "GetDocument", &json!({"Name": "Doc1"}));
    assert_eq!(needs[0].policy.as_deref(), Some(policy.as_str()));
}

#[test]
fn the_authorizer_is_handed_the_policy_and_the_evaluator_reads_it_as_the_resource_layer() {
    let mut w = World::new();
    advanced(&mut w);
    let request = json!({"Name": "/shared/db"});
    assert!(asked(&w, "GetParameter", &request)[0].policy.is_none());
    put(&mut w, &json!({})).unwrap();
    let deny = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Deny", "Principal": "*",
        "Action": "ssm:DeleteParameter", "Resource": ARN}]})
    .to_string();
    put(&mut w, &json!({"Policy": deny})).unwrap();
    let get = asked(&w, "GetParameter", &request);
    let by_arn = asked(&w, "GetParameter", &json!({"Name": format!("{ARN}:1")}));
    assert_eq!(get[0].resource, ARN);
    assert_eq!(by_arn[0].resource, ARN);
    assert_eq!(by_arn[0].policy, get[0].policy);
    let decide = |need: &Need| {
        let doc = aws_iam_policy::parse(need.policy.as_deref().unwrap())
            .unwrap()
            .unwrap();
        let resource = [&doc];
        let subject = Subject {
            arns: vec![USER.to_owned()],
            account: "000000000000".to_owned(),
            service: None,
        };
        let context = Context::new();
        let policies = Policies {
            identity: &[],
            resource: Some(&resource),
            ..Policies::default()
        };
        aws_iam_policy::evaluate(
            &policies,
            &Request {
                action: &need.action,
                resource: &need.resource,
                context: &context,
                subject: &subject,
            },
        )
        .allowed()
    };
    // The user has no identity policy: the parameter's own policy lets them read it, and its deny stops a delete.
    assert!(decide(&get[0]));
    let delete = asked(&w, "DeleteParameter", &request);
    assert!(!decide(&delete[0]));
}
