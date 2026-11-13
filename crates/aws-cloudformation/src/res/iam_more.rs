//! The other `AWS::IAM::*` types: `Policy`, `RolePolicy`, `UserPolicy`, `GroupPolicy`, `ManagedPolicy`, `AccessKey`,
//! `UserToGroupAddition`, `InstanceProfile`, `ServiceLinkedRole`, `OIDCProvider` and `SAMLProvider`.

use serde_json::{Value, json};

use super::iam::{GROUP, Kind, ROLE, SVC, USER};
use super::{Done, Made, Rc, document, gone, text};
use crate::j::J;
use crate::model::Res;

fn names(p: &Value, key: &str) -> Vec<String> {
    p.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The identities a policy resource puts its document on.
fn targets(p: &Value) -> Vec<(Kind, String)> {
    let mut out = Vec::new();
    for (kind, key) in [(ROLE, "Roles"), (USER, "Users"), (GROUP, "Groups")] {
        out.extend(names(p, key).into_iter().map(|n| (kind, n)));
    }
    for (kind, key) in [(ROLE, "RoleName"), (USER, "UserName"), (GROUP, "GroupName")] {
        if let Some(n) = text(p, key) {
            out.push((kind, n));
        }
    }
    out
}

fn same(a: &(Kind, String), b: &(Kind, String)) -> bool {
    a.0.noun == b.0.noun && a.1 == b.1
}

/// Calls IAM; a thing already gone counts as done.
async fn call_or_gone(rc: &Rc, op: &str, body: Value) -> Result<(), String> {
    match rc.call(SVC, op, body).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}

/// Calls IAM.
async fn call(rc: &Rc, op: &str, body: Value) -> Result<Value, String> {
    rc.call(SVC, op, body).await.map_err(|e| rc.fail(SVC, &e))
}

fn is_inline(ty: &str) -> bool {
    matches!(
        ty,
        "AWS::IAM::Policy"
            | "AWS::IAM::RolePolicy"
            | "AWS::IAM::UserPolicy"
            | "AWS::IAM::GroupPolicy"
    )
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    match rc.ty.as_str() {
        t if is_inline(t) => {
            put_inline(rc, &Value::Null, p).await?;
            let physical = if t == "AWS::IAM::Policy" {
                super::generated_name(rc, 128, false)
            } else {
                text(p, "PolicyName").unwrap_or_default()
            };
            Ok(Made::new(physical).with("Id", super::generated_name(rc, 128, false)))
        }
        "AWS::IAM::ManagedPolicy" => managed_policy(rc, p).await,
        "AWS::IAM::AccessKey" => access_key(rc, p).await,
        "AWS::IAM::UserToGroupAddition" => {
            members(rc, &Value::Null, p).await?;
            Ok(Made::new(super::generated_name(rc, 128, false)))
        }
        "AWS::IAM::InstanceProfile" => instance_profile(rc, p).await,
        _ => create_identity_provider(rc, p).await,
    }
}

async fn access_key(rc: &Rc, p: &Value) -> Done {
    let made = call(
        rc,
        "CreateAccessKey",
        json!({"UserName": text(p, "UserName")}),
    )
    .await?;
    let key = made.at("AccessKey");
    let id = key
        .at("AccessKeyId")
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if text(p, "Status").as_deref() == Some("Inactive") {
        let body =
            json!({"UserName": text(p, "UserName"), "AccessKeyId": id, "Status": "Inactive"});
        call(rc, "UpdateAccessKey", body).await?;
    }
    Ok(Made::new(&id).with("SecretAccessKey", key.at("SecretAccessKey").clone()))
}

async fn create_identity_provider(rc: &Rc, p: &Value) -> Done {
    match rc.ty.as_str() {
        "AWS::IAM::ServiceLinkedRole" => {
            let mut body = json!({"AWSServiceName": text(p, "AWSServiceName")});
            for k in ["CustomSuffix", "Description"] {
                if let Some(v) = text(p, k) {
                    body.put(k, v.into());
                }
            }
            let made = call(rc, "CreateServiceLinkedRole", body).await?;
            Ok(Made::new(
                made.at("Role").at("RoleName").as_str().unwrap_or_default(),
            ))
        }
        "AWS::IAM::OIDCProvider" => {
            let body = json!({"Url": text(p, "Url"), "ClientIDList": names(p, "ClientIdList"),
                "ThumbprintList": names(p, "ThumbprintList")});
            let made = call(rc, "CreateOpenIDConnectProvider", body).await?;
            let arn = made
                .at("OpenIDConnectProviderArn")
                .as_str()
                .unwrap_or_default()
                .to_owned();
            Ok(Made::new(&arn).with("Arn", arn.clone()))
        }
        "AWS::IAM::SAMLProvider" => {
            let name = text(p, "Name").unwrap_or_else(|| super::generated_name(rc, 128, false));
            let body =
                json!({"Name": name, "SAMLMetadataDocument": text(p, "SamlMetadataDocument")});
            let made = call(rc, "CreateSAMLProvider", body).await?;
            let arn = made
                .at("SAMLProviderArn")
                .as_str()
                .unwrap_or_default()
                .to_owned();
            Ok(Made::new(&arn).with("Arn", arn.clone()))
        }
        other => Err(format!("{other} is not a type doze-aws makes")),
    }
}

/// An inline policy put on (or, with `p` null, taken off) each identity it names; identities `before` named and `p` does not lose it.
async fn put_inline(rc: &Rc, before: &Value, p: &Value) -> Result<(), String> {
    let (then, now) = (targets(before), targets(p));
    let old_name = text(before, "PolicyName").unwrap_or_default();
    let name = text(p, "PolicyName").unwrap_or_default();
    for t in &then {
        if name != old_name || !now.iter().any(|n| same(n, t)) {
            let op = format!("Delete{}Policy", t.0.noun);
            call_or_gone(rc, &op, json!({t.0.name: t.1, "PolicyName": old_name})).await?;
        }
    }
    let doc = document(p, "PolicyDocument").unwrap_or_default();
    for (kind, who) in &now {
        let body = json!({kind.name: who, "PolicyName": name, "PolicyDocument": doc});
        call(rc, &format!("Put{}Policy", kind.noun), body).await?;
    }
    Ok(())
}

async fn managed_policy(rc: &Rc, p: &Value) -> Done {
    let name =
        text(p, "ManagedPolicyName").unwrap_or_else(|| super::generated_name(rc, 128, false));
    let mut body = json!({"PolicyName": name, "PolicyDocument": document(p, "PolicyDocument").unwrap_or_default()});
    for k in ["Path", "Description"] {
        if let Some(v) = text(p, k) {
            body.put(k, v.into());
        }
    }
    let made = call(rc, "CreatePolicy", body).await?;
    let arn = made
        .at("Policy")
        .at("Arn")
        .as_str()
        .unwrap_or_default()
        .to_owned();
    attach(rc, &arn, &Value::Null, p).await?;
    Ok(Made::new(&arn)
        .with("PolicyArn", arn.clone())
        .with("PolicyId", made.at("Policy").at("PolicyId").clone()))
}

/// A managed policy attached to the identities `p` names and detached from those only `before` named.
async fn attach(rc: &Rc, arn: &str, before: &Value, p: &Value) -> Result<(), String> {
    let (then, now) = (targets(before), targets(p));
    for t in then.iter().filter(|t| !now.iter().any(|n| same(n, t))) {
        call_or_gone(
            rc,
            &format!("Detach{}Policy", t.0.noun),
            json!({t.0.name: t.1, "PolicyArn": arn}),
        )
        .await?;
    }
    for t in now.iter().filter(|t| !then.iter().any(|n| same(n, t))) {
        call(
            rc,
            &format!("Attach{}Policy", t.0.noun),
            json!({t.0.name: t.1, "PolicyArn": arn}),
        )
        .await?;
    }
    Ok(())
}

async fn instance_profile(rc: &Rc, p: &Value) -> Done {
    let name =
        text(p, "InstanceProfileName").unwrap_or_else(|| super::generated_name(rc, 128, false));
    let mut body = json!({"InstanceProfileName": name});
    if let Some(path) = text(p, "Path") {
        body.put("Path", path.into());
    }
    let made = call(rc, "CreateInstanceProfile", body).await?;
    roles(rc, &name, &Value::Null, p).await?;
    Ok(Made::new(&name).with("Arn", made.at("InstanceProfile").at("Arn").clone()))
}

/// An instance profile's roles: added and removed to match the template.
async fn roles(rc: &Rc, profile: &str, before: &Value, p: &Value) -> Result<(), String> {
    let (then, now) = (names(before, "Roles"), names(p, "Roles"));
    for r in then.iter().filter(|r| !now.contains(r)) {
        call_or_gone(
            rc,
            "RemoveRoleFromInstanceProfile",
            json!({"InstanceProfileName": profile, "RoleName": r}),
        )
        .await?;
    }
    for r in now.iter().filter(|r| !then.contains(r)) {
        call(
            rc,
            "AddRoleToInstanceProfile",
            json!({"InstanceProfileName": profile, "RoleName": r}),
        )
        .await?;
    }
    Ok(())
}

/// A group's members as a `UserToGroupAddition` names them.
async fn members(rc: &Rc, before: &Value, p: &Value) -> Result<(), String> {
    let group = text(p, "GroupName")
        .or_else(|| text(before, "GroupName"))
        .unwrap_or_default();
    let (then, now) = (names(before, "Users"), names(p, "Users"));
    for u in then.iter().filter(|u| !now.contains(u)) {
        call_or_gone(
            rc,
            "RemoveUserFromGroup",
            json!({"GroupName": group, "UserName": u}),
        )
        .await?;
    }
    for u in now.iter().filter(|u| !then.contains(u)) {
        call(
            rc,
            "AddUserToGroup",
            json!({"GroupName": group, "UserName": u}),
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let physical = old.physical.clone().unwrap_or_default();
    let before = old.props_value();
    match rc.ty.as_str() {
        t if is_inline(t) => put_inline(rc, &before, p).await?,
        "AWS::IAM::ManagedPolicy" => {
            if document(&before, "PolicyDocument") != document(p, "PolicyDocument") {
                new_version(rc, &physical, p).await?;
            }
            attach(rc, &physical, &before, p).await?;
        }
        "AWS::IAM::AccessKey" => {
            let status = text(p, "Status").unwrap_or_else(|| "Active".into());
            let body =
                json!({"UserName": text(p, "UserName"), "AccessKeyId": physical, "Status": status});
            call(rc, "UpdateAccessKey", body).await?;
        }
        "AWS::IAM::UserToGroupAddition" => members(rc, &before, p).await?,
        "AWS::IAM::InstanceProfile" => roles(rc, &physical, &before, p).await?,
        _ => {}
    }
    let mut made = Made::new(&physical);
    made.attrs = old.attrs_map();
    Ok(made)
}

/// A managed policy's new document as its default version, making room when it already has five.
async fn new_version(rc: &Rc, arn: &str, p: &Value) -> Result<(), String> {
    if let Ok(listed) = rc
        .call(SVC, "ListPolicyVersions", json!({"PolicyArn": arn}))
        .await
    {
        let versions = listed
            .at("Versions")
            .as_array()
            .cloned()
            .unwrap_or_default();
        if versions.len() >= 5
            && let Some(oldest) = versions
                .iter()
                .rfind(|v| v.at("IsDefaultVersion") != &Value::Bool(true))
        {
            let body = json!({"PolicyArn": arn, "VersionId": oldest.at("VersionId")});
            call_or_gone(rc, "DeletePolicyVersion", body).await?;
        }
    }
    let body = json!({"PolicyArn": arn, "PolicyDocument": document(p, "PolicyDocument"), "SetAsDefault": true});
    call(rc, "CreatePolicyVersion", body).await.map(|_| ())
}

/// A managed policy's versions but the default, deleted (IAM deletes a policy only when it has no other).
async fn drop_versions(rc: &Rc, arn: &str) -> Result<(), String> {
    if let Ok(listed) = rc
        .call(SVC, "ListPolicyVersions", json!({"PolicyArn": arn}))
        .await
    {
        for v in listed.at("Versions").as_array().into_iter().flatten() {
            if v.at("IsDefaultVersion") != &Value::Bool(true) {
                call_or_gone(
                    rc,
                    "DeletePolicyVersion",
                    json!({"PolicyArn": arn, "VersionId": v.at("VersionId")}),
                )
                .await?;
            }
        }
    }
    Ok(())
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let id = res.physical.clone().unwrap_or_default();
    let before = res.props_value();
    let (op, body) = match rc.ty.as_str() {
        t if is_inline(t) => {
            return put_inline(
                rc,
                &before,
                &json!({"PolicyName": text(&before, "PolicyName")}),
            )
            .await;
        }
        "AWS::IAM::ManagedPolicy" => {
            attach(rc, &id, &before, &Value::Null).await?;
            drop_versions(rc, &id).await?;
            ("DeletePolicy", json!({"PolicyArn": id}))
        }
        "AWS::IAM::AccessKey" => (
            "DeleteAccessKey",
            json!({"UserName": text(&before, "UserName"), "AccessKeyId": id}),
        ),
        "AWS::IAM::UserToGroupAddition" => {
            return members(
                rc,
                &before,
                &json!({"GroupName": text(&before, "GroupName")}),
            )
            .await;
        }
        "AWS::IAM::InstanceProfile" => {
            roles(rc, &id, &before, &Value::Null).await?;
            ("DeleteInstanceProfile", json!({"InstanceProfileName": id}))
        }
        "AWS::IAM::ServiceLinkedRole" => ("DeleteServiceLinkedRole", json!({"RoleName": id})),
        "AWS::IAM::OIDCProvider" => (
            "DeleteOpenIDConnectProvider",
            json!({"OpenIDConnectProviderArn": id}),
        ),
        "AWS::IAM::SAMLProvider" => ("DeleteSAMLProvider", json!({"SAMLProviderArn": id})),
        _ => return Ok(()),
    };
    call_or_gone(rc, op, body).await
}
