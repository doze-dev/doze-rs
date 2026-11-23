//! A function made for a test: its code zipped with the `zip` program as a person would.
#![allow(dead_code)]

use serde_json::json;

use crate::world::World;

/// Makes the function `name` (and the role it needs). `false` when there is no `zip` program: the test then does nothing.
pub(crate) async fn function(w: &World, name: &str) -> bool {
    let have = std::process::Command::new("sh")
        .args(["-c", "command -v zip"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !have {
        return false;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bootstrap");
    std::fs::write(&file, "#!/bin/sh\n").unwrap();
    let zip = dir.path().join("f.zip");
    let ok = std::process::Command::new("zip")
        .args(["-qj", zip.to_str().unwrap(), file.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(ok.success());
    let trust = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow",
        "Principal": {"Service": "lambda.amazonaws.com"}, "Action": "sts:AssumeRole"}]});
    w.app(
        "iam",
        "CreateRole",
        &json!({"RoleName": "fn-role", "AssumeRolePolicyDocument": trust.to_string()}),
    )
    .await;
    // The role may read queues (a mapping from a queue is refused when it may not).
    let allow = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": "sqs:*", "Resource": "*"}]});
    w.app(
        "iam",
        "PutRolePolicy",
        &json!({"RoleName": "fn-role", "PolicyName": "queues", "PolicyDocument": allow.to_string()}),
    )
    .await;
    w.app(
        "lambda",
        "CreateFunction",
        &json!({"FunctionName": name, "Runtime": "provided.al2023", "Handler": "x",
            "Role": "arn:aws:iam::000000000000:role/fn-role",
            "Code": {"ZipFile": aws_core::base64::encode(&std::fs::read(&zip).unwrap())}}),
    )
    .await;
    true
}

/// Another function `name` in a world whose first ([`function`]) made the role.
pub(crate) async fn function_again(w: &World, name: &str) -> bool {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bootstrap");
    std::fs::write(&file, "#!/bin/sh\n").unwrap();
    let zip = dir.path().join("f.zip");
    let ok = std::process::Command::new("zip")
        .args(["-qj", zip.to_str().unwrap(), file.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(ok.success());
    w.app(
        "lambda",
        "CreateFunction",
        &json!({"FunctionName": name, "Runtime": "provided.al2023", "Handler": "x",
            "Role": "arn:aws:iam::000000000000:role/fn-role",
            "Code": {"ZipFile": aws_core::base64::encode(&std::fs::read(&zip).unwrap())}}),
    )
    .await;
    true
}
