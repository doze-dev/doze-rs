//! Managed policies and their versions.

use std::collections::BTreeMap;

use aws_iam_policy::Kind as Doc;
use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::entity::{arn_of, new_id};
use crate::errors::{conflict, limit, malformed, no_such_policy};
use crate::model::{Managed, Version};
use crate::req::{Req, encoded, page, secs, tag_list};
use crate::view;

const MAX_VERSIONS: usize = 5;
const AWS_PREFIX: &str = "arn:aws:iam::aws:policy/";
const EMPTY_DOCUMENT: &str = r#"{"Version":"2012-10-17","Statement":[]}"#;
/// The one AWS-managed policy whose document is the same everywhere and known without asking AWS.
const ADMINISTRATOR: &str =
    r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"*","Resource":"*"}]}"#;

impl Core {
    /// The policy `arn` names. An AWS-managed policy (`arn:aws:iam::aws:policy/...`) is made the first time it is named. Only
    /// `AdministratorAccess` has its document; the rest are empty, since doze-aws does not carry AWS's documents yet (they are to be
    /// recorded from the AWS account): such a policy attaches and lists but allows nothing, which enforce mode would show as a
    /// refusal that AWS would not make.
    pub(crate) fn known_policy(&mut self, arn: &str) -> Result<(), ApiError> {
        if self.policies.get(arn).is_some() {
            return Ok(());
        }
        let Some(rest) = arn.strip_prefix(AWS_PREFIX) else {
            return Err(no_such_policy(arn));
        };
        let (path, name) = rest
            .rsplit_once('/')
            .map_or(("/".to_owned(), rest), |(p, n)| (format!("/{p}/"), n));
        let now = self.now_ms();
        let p = Managed {
            arn: arn.to_owned(),
            name: name.to_owned(),
            path,
            id: new_id("ANPA")?,
            description: None,
            created_ms: now,
            updated_ms: now,
            versions: vec![Version {
                id: "v1".into(),
                document: if name == "AdministratorAccess" {
                    ADMINISTRATOR
                } else {
                    EMPTY_DOCUMENT
                }
                .into(),
                created_ms: now,
            }],
            default: "v1".into(),
            next_version: 2,
            tags: BTreeMap::new(),
            aws_managed: true,
        };
        self.policies.insert(arn.to_owned(), p);
        Ok(())
    }

    fn policy_named(&self, arn: &str) -> Result<&Managed, ApiError> {
        self.policies.get(arn).ok_or_else(|| no_such_policy(arn))
    }

    /// How many entities have the policy attached, and how many use it as a boundary.
    fn uses(&self, arn: &str) -> (usize, usize) {
        let attached = self
            .entities
            .iter()
            .filter(|(_, e)| e.attached.contains(arn))
            .count();
        let bounded = self
            .entities
            .iter()
            .filter(|(_, e)| e.boundary.as_deref() == Some(arn))
            .count();
        (attached, bounded)
    }

    pub(crate) fn create_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("PolicyName");
        let path = r.opt("Path").unwrap_or("/");
        let arn = arn_of(r.account(), "policy", path, name);
        if self.policies.get(&arn).is_some() {
            // (memory)
            return Err(ApiError::modeled(
                "EntityAlreadyExistsException",
                format!("A policy called {name} already exists. Duplicate names are not allowed."),
            ));
        }
        let text = r.s("PolicyDocument");
        aws_iam_policy::check(text, Doc::Identity).map_err(|e| malformed(e.0))?;
        let now = self.now_ms();
        let p = Managed {
            arn: arn.clone(),
            name: name.to_owned(),
            path: path.to_owned(),
            id: new_id("ANPA")?,
            description: r.opt("Description").map(str::to_owned),
            created_ms: now,
            updated_ms: now,
            versions: vec![Version {
                id: "v1".into(),
                document: text.to_owned(),
                created_ms: now,
            }],
            default: "v1".into(),
            next_version: 2,
            tags: r.tags().into_iter().collect(),
            aws_managed: false,
        };
        let shown = view::policy(&p, 0, 0, false);
        self.policies.insert(arn, p);
        Ok(json!({"Policy": shown}))
    }

    pub(crate) fn get_policy(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let p = self.policy_named(r.s("PolicyArn"))?;
        let (attached, bounded) = self.uses(&p.arn);
        Ok(json!({"Policy": view::policy(p, attached, bounded, true)}))
    }

    pub(crate) fn delete_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let p = self.policy_named(r.s("PolicyArn"))?;
        let (attached, bounded) = self.uses(&p.arn);
        if attached + bounded > 0 {
            return Err(conflict("Cannot delete a policy attached to entities."));
        }
        if p.versions.len() > 1 {
            // (memory)
            return Err(conflict(
                "Cannot delete a policy with non-default versions: delete them first.",
            ));
        }
        let arn = p.arn.clone();
        self.policies.remove(&arn);
        Ok(Value::Null)
    }

    pub(crate) fn list_policies(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let scope = r.opt("Scope").unwrap_or("All");
        let prefix = r.opt("PathPrefix").unwrap_or("/");
        let only_attached = r.flag("OnlyAttached").unwrap_or(false);
        let items = self
            .policies
            .iter()
            .map(|(_, p)| p)
            .filter(|p| match scope {
                "AWS" => p.aws_managed,
                "Local" => !p.aws_managed,
                _ => true,
            })
            .filter(|p| p.path.starts_with(prefix))
            .map(|p| (p, self.uses(&p.arn)))
            .filter(|(_, (attached, _))| !only_attached || *attached > 0)
            .map(|(p, (attached, bounded))| view::policy(p, attached, bounded, false))
            .collect();
        page(r, items, "Policies")
    }

    pub(crate) fn create_policy_version(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let p = self.policy_named(r.s("PolicyArn"))?;
        let text = r.s("PolicyDocument");
        aws_iam_policy::check(text, Doc::Identity).map_err(|e| malformed(e.0))?;
        if p.versions.len() >= MAX_VERSIONS {
            // (memory)
            return Err(limit(
                "A managed policy can have up to 5 versions. Before you create a new version, you must delete an existing version.",
            ));
        }
        let now = self.now_ms();
        let arn = p.arn.clone();
        let make_default = r.flag("SetAsDefault").unwrap_or(false);
        let Some(p) = self.policies.get_mut(&arn) else {
            return Err(no_such_policy(&arn));
        };
        let id = format!("v{}", p.next_version);
        p.next_version += 1;
        p.versions.push(Version {
            id: id.clone(),
            document: text.to_owned(),
            created_ms: now,
        });
        if make_default {
            p.default.clone_from(&id);
            p.updated_ms = now;
        }
        Ok(
            json!({"PolicyVersion": {"VersionId": id, "IsDefaultVersion": make_default, "CreateDate": secs(now)}}),
        )
    }

    pub(crate) fn get_policy_version(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let p = self.policy_named(r.s("PolicyArn"))?;
        let v = p
            .versions
            .iter()
            .find(|v| v.id == r.s("VersionId"))
            .ok_or_else(|| no_such_version(&p.arn, r.s("VersionId")))?;
        Ok(json!({"PolicyVersion": {
            "Document": encoded(&v.document), "VersionId": v.id, "IsDefaultVersion": v.id == p.default, "CreateDate": secs(v.created_ms)
        }}))
    }

    pub(crate) fn list_policy_versions(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let p = self.policy_named(r.s("PolicyArn"))?;
        let items = p
            .versions
            .iter()
            .rev()
            .map(|v| json!({"VersionId": v.id, "IsDefaultVersion": v.id == p.default, "CreateDate": secs(v.created_ms)}))
            .collect();
        page(r, items, "Versions")
    }

    pub(crate) fn delete_policy_version(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let arn = r.s("PolicyArn");
        let p = self.policy_named(arn)?;
        let id = r.s("VersionId");
        if p.default == id {
            return Err(conflict("Cannot delete the default version of a policy."));
        }
        if !p.versions.iter().any(|v| v.id == id) {
            return Err(no_such_version(arn, id));
        }
        if let Some(p) = self.policies.get_mut(arn) {
            p.versions.retain(|v| v.id != id);
        }
        Ok(Value::Null)
    }

    pub(crate) fn set_default_policy_version(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let arn = r.s("PolicyArn");
        let p = self.policy_named(arn)?;
        let id = r.s("VersionId");
        if !p.versions.iter().any(|v| v.id == id) {
            return Err(no_such_version(arn, id));
        }
        let now = self.now_ms();
        if let Some(p) = self.policies.get_mut(arn) {
            p.default = id.to_owned();
            p.updated_ms = now;
        }
        Ok(Value::Null)
    }

    pub(crate) fn tag_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let new = r.tags();
        self.policy_named(r.s("PolicyArn"))?;
        if let Some(p) = self.policies.get_mut(r.s("PolicyArn")) {
            p.tags.extend(new);
        }
        Ok(Value::Null)
    }

    pub(crate) fn untag_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        self.policy_named(r.s("PolicyArn"))?;
        if let Some(p) = self.policies.get_mut(r.s("PolicyArn")) {
            for k in r.strings("TagKeys") {
                p.tags.remove(&k);
            }
        }
        Ok(Value::Null)
    }

    pub(crate) fn list_policy_tags(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let p = self.policy_named(r.s("PolicyArn"))?;
        let Value::Array(items) = tag_list(p.tags.iter()) else {
            return Ok(Value::Null);
        };
        page(r, items, "Tags")
    }

    pub(crate) fn list_entities_for_policy(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let arn = r.s("PolicyArn");
        self.policy_named(arn)?;
        let filter = r.opt("EntityFilter");
        let mut users = Vec::new();
        let mut groups = Vec::new();
        let mut roles = Vec::new();
        for (_, e) in self
            .entities
            .iter()
            .filter(|(_, e)| e.attached.contains(arn))
        {
            match e.kind {
                crate::model::Kind::User if filter.is_none_or(|f| f == "User") => {
                    users.push(json!({"UserName": e.name, "UserId": e.id}))
                }
                crate::model::Kind::Group if filter.is_none_or(|f| f == "Group") => {
                    groups.push(json!({"GroupName": e.name, "GroupId": e.id}))
                }
                crate::model::Kind::Role if filter.is_none_or(|f| f == "Role") => {
                    roles.push(json!({"RoleName": e.name, "RoleId": e.id}))
                }
                _ => {}
            }
        }
        Ok(
            json!({"PolicyUsers": users, "PolicyGroups": groups, "PolicyRoles": roles, "IsTruncated": false}),
        )
    }
}

fn no_such_version(arn: &str, id: &str) -> ApiError {
    // (memory)
    ApiError::modeled(
        "NoSuchEntityException",
        format!("Policy {arn} version {id} was not found."),
    )
}
