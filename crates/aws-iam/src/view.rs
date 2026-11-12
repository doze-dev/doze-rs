//! The JSON IAM writes for each thing it holds. Create calls leave some members out that Get calls have (recorded 2026-11-09: a
//! `CreateRole` answer has no `Description`, `MaxSessionDuration` or `RoleLastUsed`; a `GetPolicy` has `Tags` and a `CreatePolicy`
//! has not).

use serde_json::{Map, Value, json};

use crate::model::{Entity, Kind, Managed, Profile};
use crate::req::{encoded, secs, tag_list};

pub(crate) fn boundary(arn: &str) -> Value {
    json!({"PermissionsBoundaryType": "PermissionsBoundaryPolicy", "PermissionsBoundaryArn": arn})
}

/// A user, group or role as its `Create`, `Get` and `List` calls show it. `full` is a `Get`.
pub(crate) fn entity(e: &Entity, full: bool) -> Value {
    let title = e.kind.title();
    let mut m = Map::new();
    m.insert("Path".into(), json!(e.path));
    m.insert(format!("{title}Name"), json!(e.name));
    m.insert(format!("{title}Id"), json!(e.id));
    m.insert("Arn".into(), json!(e.arn));
    m.insert("CreateDate".into(), secs(e.created_ms));
    if e.kind == Kind::Role {
        m.insert(
            "AssumeRolePolicyDocument".into(),
            json!(encoded(e.trust.as_deref().unwrap_or_default())),
        );
        if full {
            if let Some(d) = &e.description {
                m.insert("Description".into(), json!(d));
            }
            m.insert("MaxSessionDuration".into(), json!(e.max_session_secs));
            if let Some((at, region)) = &e.last_used {
                m.insert(
                    "RoleLastUsed".into(),
                    json!({"LastUsedDate": secs(*at), "Region": region}),
                );
            }
        }
    }
    if let Some(b) = &e.boundary {
        m.insert("PermissionsBoundary".into(), boundary(b));
    }
    if e.kind != Kind::Group && (full || !e.tags.is_empty()) {
        m.insert("Tags".into(), tag_list(e.tags.iter()));
    }
    Value::Object(m)
}

/// A managed policy. `full` is a `Get` (which has `Tags`).
pub(crate) fn policy(p: &Managed, attachments: usize, boundary_uses: usize, full: bool) -> Value {
    let mut m = Map::new();
    m.insert("PolicyName".into(), json!(p.name));
    m.insert("PolicyId".into(), json!(p.id));
    m.insert("Arn".into(), json!(p.arn));
    m.insert("Path".into(), json!(p.path));
    m.insert("DefaultVersionId".into(), json!(p.default));
    m.insert("AttachmentCount".into(), json!(attachments));
    m.insert("PermissionsBoundaryUsageCount".into(), json!(boundary_uses));
    m.insert("IsAttachable".into(), json!(true));
    if let Some(d) = &p.description {
        m.insert("Description".into(), json!(d));
    }
    m.insert("CreateDate".into(), secs(p.created_ms));
    m.insert("UpdateDate".into(), secs(p.updated_ms));
    if full {
        m.insert("Tags".into(), tag_list(p.tags.iter()));
    }
    Value::Object(m)
}

/// An instance profile. `full` is a `Get` (which has `Tags`).
pub(crate) fn profile(p: &Profile, roles: Vec<Value>, full: bool) -> Value {
    let mut m = Map::new();
    m.insert("Path".into(), json!(p.path));
    m.insert("InstanceProfileName".into(), json!(p.name));
    m.insert("InstanceProfileId".into(), json!(p.id));
    m.insert("Arn".into(), json!(p.arn));
    m.insert("CreateDate".into(), secs(p.created_ms));
    m.insert("Roles".into(), Value::Array(roles));
    if full {
        m.insert("Tags".into(), tag_list(p.tags.iter()));
    }
    Value::Object(m)
}
