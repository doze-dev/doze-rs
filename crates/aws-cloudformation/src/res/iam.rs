//! `AWS::IAM::Role`, `User` and `Group` into the IAM store (so the policy evaluator sees them), with their inline policies, managed
//! policies, boundaries and tags. The other IAM types are in `iam_more`.

use serde_json::{Value, json};

use super::{Done, Made, Rc, document, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

pub(super) const SVC: &str = "iam";

/// The three kinds of identity, as IAM's operations spell them.
#[derive(Clone, Copy)]
pub(super) struct Kind {
    pub(super) noun: &'static str,
    pub(super) name: &'static str,
}

pub(super) const ROLE: Kind = Kind {
    noun: "Role",
    name: "RoleName",
};
pub(super) const USER: Kind = Kind {
    noun: "User",
    name: "UserName",
};
pub(super) const GROUP: Kind = Kind {
    noun: "Group",
    name: "GroupName",
};

fn kind_of(ty: &str) -> Option<Kind> {
    match ty {
        "AWS::IAM::Role" => Some(ROLE),
        "AWS::IAM::User" => Some(USER),
        "AWS::IAM::Group" => Some(GROUP),
        _ => None,
    }
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    let Some(kind) = kind_of(&rc.ty) else {
        return super::iam_more::create(rc, p).await;
    };
    let name = text(p, kind.name).unwrap_or_else(|| super::generated_name(rc, 64, false));
    if rc
        .call(SVC, &format!("Get{}", kind.noun), json!({kind.name: name}))
        .await
        .is_ok()
    {
        return Err(rc.taken(&name));
    }
    let mut body = json!({kind.name: name});
    for k in [
        "Path",
        "Description",
        "MaxSessionDuration",
        "PermissionsBoundary",
    ] {
        if let Some(v) = text(p, k) {
            body.put(k, v.into());
        }
    }
    if kind.noun == "Role" {
        body.put(
            "AssumeRolePolicyDocument",
            document(p, "AssumeRolePolicyDocument")
                .unwrap_or_default()
                .into(),
        );
    }
    if kind.noun == "Group" {
        if let Some(o) = body.as_object_mut() {
            o.retain(|k, _| k == "GroupName" || k == "Path");
        }
    } else {
        let tags: Vec<Value> = tag_list(rc, p)
            .into_iter()
            .map(|(k, v)| json!({"Key": k, "Value": v}))
            .collect();
        if !tags.is_empty() {
            body.put("Tags", Value::Array(tags));
        }
    }
    let made = rc
        .call(SVC, &format!("Create{}", kind.noun), body)
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    let entity = &made.at(kind.noun);
    inline(rc, kind, &name, &Value::Null, p).await?;
    managed(rc, kind, &name, &Value::Null, p).await?;
    if kind.noun == "User" {
        user_extras(rc, &name, &Value::Null, p).await?;
    }
    Ok(described(kind, &name, entity))
}

fn described(kind: Kind, name: &str, entity: &Value) -> Made {
    let mut m = Made::new(name).with("Arn", entity.at("Arn").clone());
    let id = format!("{}Id", kind.noun);
    if let Some(v) = entity.get(&id) {
        m = m.with(&id, v.clone());
    }
    m
}

/// The inline policies (`Policies`): put what is new or changed, delete what went.
async fn inline(rc: &Rc, kind: Kind, name: &str, before: &Value, p: &Value) -> Result<(), String> {
    let list = |v: &Value| -> Vec<(String, String)> {
        v.get("Policies")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|x| {
                        (
                            text(x, "PolicyName").unwrap_or_default(),
                            document(x, "PolicyDocument").unwrap_or_default(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let (then, now) = (list(before), list(p));
    for (pn, _) in then
        .iter()
        .filter(|(n, _)| !now.iter().any(|(m, _)| m == n))
    {
        match rc
            .call(
                SVC,
                &format!("Delete{}Policy", kind.noun),
                json!({kind.name: name, "PolicyName": pn}),
            )
            .await
        {
            Ok(_) => {}
            Err(e) if gone(&e) => {}
            Err(e) => return Err(rc.fail(SVC, &e)),
        }
    }
    for (pn, doc) in now.iter().filter(|x| !then.contains(x)) {
        rc.call(
            SVC,
            &format!("Put{}Policy", kind.noun),
            json!({kind.name: name, "PolicyName": pn, "PolicyDocument": doc}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    }
    Ok(())
}

/// The managed policies (`ManagedPolicyArns`): attach what is new, detach what went.
async fn managed(rc: &Rc, kind: Kind, name: &str, before: &Value, p: &Value) -> Result<(), String> {
    let list = |v: &Value| -> Vec<String> {
        v.get("ManagedPolicyArns")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let (then, now) = (list(before), list(p));
    for arn in then.iter().filter(|a| !now.contains(a)) {
        match rc
            .call(
                SVC,
                &format!("Detach{}Policy", kind.noun),
                json!({kind.name: name, "PolicyArn": arn}),
            )
            .await
        {
            Ok(_) => {}
            Err(e) if gone(&e) => {}
            Err(e) => return Err(rc.fail(SVC, &e)),
        }
    }
    for arn in now.iter().filter(|a| !then.contains(a)) {
        rc.call(
            SVC,
            &format!("Attach{}Policy", kind.noun),
            json!({kind.name: name, "PolicyArn": arn}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    }
    Ok(())
}

/// A user's groups and console password.
async fn user_extras(rc: &Rc, name: &str, before: &Value, p: &Value) -> Result<(), String> {
    let groups = |v: &Value| -> Vec<String> {
        v.get("Groups")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let (then, now) = (groups(before), groups(p));
    for g in then.iter().filter(|g| !now.contains(g)) {
        drop(
            rc.call(
                SVC,
                "RemoveUserFromGroup",
                json!({"UserName": name, "GroupName": g}),
            )
            .await,
        );
    }
    for g in now.iter().filter(|g| !then.contains(g)) {
        rc.call(
            SVC,
            "AddUserToGroup",
            json!({"UserName": name, "GroupName": g}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    }
    if let Some(lp) = p
        .get("LoginProfile")
        .filter(|l| Some(*l) != before.get("LoginProfile"))
    {
        let body = json!({"UserName": name, "Password": text(lp, "Password"),
            "PasswordResetRequired": text(lp, "PasswordResetRequired").as_deref() == Some("true")});
        let op = if before.get("LoginProfile").is_some() {
            "UpdateLoginProfile"
        } else {
            "CreateLoginProfile"
        };
        rc.call(SVC, op, body).await.map_err(|e| rc.fail(SVC, &e))?;
    }
    Ok(())
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let Some(kind) = kind_of(&rc.ty) else {
        return super::iam_more::update(rc, old, p).await;
    };
    let name = old.physical.clone().unwrap_or_default();
    let before = old.props_value();
    if kind.noun == "Role" {
        if document(&before, "AssumeRolePolicyDocument") != document(p, "AssumeRolePolicyDocument")
        {
            rc.call(SVC, "UpdateAssumeRolePolicy", json!({"RoleName": name, "PolicyDocument": document(p, "AssumeRolePolicyDocument")}))
                .await
                .map_err(|e| rc.fail(SVC, &e))?;
        }
        if text(&before, "Description") != text(p, "Description")
            || text(&before, "MaxSessionDuration") != text(p, "MaxSessionDuration")
        {
            let mut body = json!({"RoleName": name, "Description": text(p, "Description").unwrap_or_default()});
            if let Some(m) = text(p, "MaxSessionDuration") {
                body.put("MaxSessionDuration", m.into());
            }
            rc.call(SVC, "UpdateRole", body)
                .await
                .map_err(|e| rc.fail(SVC, &e))?;
        }
    }
    if kind.noun != "Group"
        && text(&before, "PermissionsBoundary") != text(p, "PermissionsBoundary")
    {
        let r = match text(p, "PermissionsBoundary") {
            Some(b) => {
                rc.call(
                    SVC,
                    &format!("Put{}PermissionsBoundary", kind.noun),
                    json!({kind.name: name, "PermissionsBoundary": b}),
                )
                .await
            }
            None => {
                rc.call(
                    SVC,
                    &format!("Delete{}PermissionsBoundary", kind.noun),
                    json!({kind.name: name}),
                )
                .await
            }
        };
        r.map_err(|e| rc.fail(SVC, &e))?;
    }
    inline(rc, kind, &name, &before, p).await?;
    managed(rc, kind, &name, &before, p).await?;
    if kind.noun == "User" {
        user_extras(rc, &name, &before, p).await?;
    }
    if kind.noun != "Group" {
        retag(rc, kind, &name, &tag_list(rc, &before), &tag_list(rc, p)).await?;
    }
    let got = rc
        .call(SVC, &format!("Get{}", kind.noun), json!({kind.name: name}))
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    Ok(described(kind, &name, got.at(kind.noun)))
}

async fn retag(
    rc: &Rc,
    kind: Kind,
    name: &str,
    then: &[(String, String)],
    now: &[(String, String)],
) -> Result<(), String> {
    if then == now {
        return Ok(());
    }
    let removed: Vec<&String> = then
        .iter()
        .map(|(k, _)| k)
        .filter(|k| !now.iter().any(|(n, _)| n == *k))
        .collect();
    if !removed.is_empty() {
        rc.call(
            SVC,
            &format!("Untag{}", kind.noun),
            json!({kind.name: name, "TagKeys": removed}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    }
    if !now.is_empty() {
        let tags: Vec<Value> = now
            .iter()
            .map(|(k, v)| json!({"Key": k, "Value": v}))
            .collect();
        rc.call(
            SVC,
            &format!("Tag{}", kind.noun),
            json!({kind.name: name, "Tags": tags}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    }
    Ok(())
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let Some(kind) = kind_of(&rc.ty) else {
        return super::iam_more::delete(rc, res).await;
    };
    let name = res.physical.clone().unwrap_or_default();
    let before = res.props_value();
    // What the template put on it comes off first: IAM refuses to delete an identity that still has policies.
    inline(rc, kind, &name, &before, &Value::Null).await?;
    managed(rc, kind, &name, &before, &Value::Null).await?;
    if kind.noun == "User" {
        user_extras(rc, &name, &before, &Value::Null).await?;
        if before.get("LoginProfile").is_some() {
            drop(
                rc.call(SVC, "DeleteLoginProfile", json!({"UserName": name}))
                    .await,
            );
        }
    }
    if kind.noun == "Group" {
        // Members added by other resources leave with the group.
        if let Ok(g) = rc.call(SVC, "GetGroup", json!({"GroupName": name})).await {
            for u in g.at("Users").as_array().into_iter().flatten() {
                drop(
                    rc.call(
                        SVC,
                        "RemoveUserFromGroup",
                        json!({"GroupName": name, "UserName": u.at("UserName")}),
                    )
                    .await,
                );
            }
        }
    }
    match rc
        .call(
            SVC,
            &format!("Delete{}", kind.noun),
            json!({kind.name: name}),
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
