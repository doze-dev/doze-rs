//! Users, groups and roles: create, get, update, delete, list, tag. One body of code for the three, since the records and most of the
//! rules are the same; what differs is the name of the members (`UserName`, `GroupName`, `RoleName`) and a role's trust policy.

use std::collections::{BTreeMap, BTreeSet};

use aws_iam_policy::Kind as Doc;
use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::{Core, entity_key};
use crate::errors::{conflict, exists, invalid_input, limit, malformed};
use crate::model::{Entity, Kind};
use crate::req::{Req, page, tag_list};
use crate::view;

const MAX_TAGS: usize = 50;
const DEFAULT_SESSION_SECS: i64 = 3600;

fn name_key(kind: Kind) -> String {
    format!("{}Name", kind.title())
}

/// A new id: four letters for the kind, seventeen of IAM's alphabet.
pub(crate) fn new_id(prefix: &str) -> Result<String, ApiError> {
    const SET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::from(prefix);
    for _ in 0..17 {
        let ix =
            aws_seal::random_below(SET.len()).ok_or_else(|| ApiError::internal("no randomness"))?;
        out.push(char::from(SET.get(ix).copied().unwrap_or(b'A')));
    }
    Ok(out)
}

/// `arn:aws:iam::<account>:<word><path><name>`; the path starts and ends with a slash.
pub(crate) fn arn_of(account: &str, word: &str, path: &str, name: &str) -> String {
    format!("arn:aws:iam::{account}:{word}{path}{name}")
}

impl Core {
    pub(crate) fn create_entity(&mut self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s(&name_key(kind)).to_owned();
        if self.entities.get(&entity_key(kind, &name)).is_some() {
            return Err(exists(kind.title(), &name));
        }
        let trust = if kind == Kind::Role {
            let text = r.s("AssumeRolePolicyDocument");
            aws_iam_policy::check(text, Doc::Trust).map_err(|e| malformed(e.0))?;
            Some(text.to_owned())
        } else {
            None
        };
        let boundary = self.checked_boundary(r.opt("PermissionsBoundary"))?;
        let tags: BTreeMap<String, String> = r.tags().into_iter().collect();
        if tags.len() > MAX_TAGS {
            return Err(limit("Cannot exceed quota for TagsPerEntity: 50"));
        }
        let max = r.int("MaxSessionDuration").unwrap_or(DEFAULT_SESSION_SECS);
        if !(DEFAULT_SESSION_SECS..=43_200).contains(&max) {
            return Err(invalid_input(
                "Invalid Session Duration: it must be between 3600 and 43200 seconds.",
            ));
        }
        let path = r.opt("Path").unwrap_or("/").to_owned();
        let entity = Entity {
            kind,
            id: new_id(kind.id_prefix())?,
            arn: arn_of(r.account(), kind.word(), &path, &name),
            name,
            path,
            created_ms: self.now_ms(),
            tags,
            inline: BTreeMap::new(),
            attached: BTreeSet::new(),
            boundary,
            trust,
            description: r.opt("Description").map(str::to_owned),
            max_session_secs: max,
            last_used: None,
            service_linked: None,
            groups: BTreeSet::new(),
            password_set_ms: None,
        };
        let shown = view::entity(&entity, false);
        self.entities.insert(entity_key(kind, &entity.name), entity);
        Ok(json!({ kind.title(): shown }))
    }

    /// A permissions boundary must be a policy that exists: the account's, or an AWS-managed one (kept from then on).
    pub(crate) fn checked_boundary(
        &mut self,
        arn: Option<&str>,
    ) -> Result<Option<String>, ApiError> {
        let Some(arn) = arn else { return Ok(None) };
        self.known_policy(arn)?;
        Ok(Some(arn.to_owned()))
    }

    pub(crate) fn get_entity(&self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s(&name_key(kind));
        let e = self.entity(kind, name)?;
        Ok(json!({ kind.title(): view::entity(e, true) }))
    }

    pub(crate) fn list_entities(&self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        let prefix = r.opt("PathPrefix").unwrap_or("/");
        let items: Vec<Value> = self
            .entities
            .with_prefix(&format!("{}/", kind.word()))
            .map(|(_, e)| e)
            .filter(|e| e.path.starts_with(prefix))
            .map(|e| view::entity(e, false))
            .collect();
        page(r, items, &format!("{}s", kind.title()))
    }

    pub(crate) fn update_role(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let description = r.opt("Description").map(str::to_owned);
        let max = r.int("MaxSessionDuration");
        if max.is_some_and(|m| !(DEFAULT_SESSION_SECS..=43_200).contains(&m)) {
            return Err(invalid_input(
                "Invalid Session Duration: it must be between 3600 and 43200 seconds.",
            ));
        }
        let role = self.entity_mut(Kind::Role, r.s("RoleName"))?;
        if description.is_some() {
            role.description = description;
        }
        if let Some(m) = max {
            role.max_session_secs = m;
        }
        Ok(Value::Null)
    }

    pub(crate) fn update_role_description(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let role = self.entity_mut(Kind::Role, r.s("RoleName"))?;
        role.description = Some(r.s("Description").to_owned());
        Ok(json!({"Role": view::entity(role, true)}))
    }

    pub(crate) fn update_assume_role_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        self.entity(Kind::Role, r.s("RoleName"))?;
        let text = r.s("PolicyDocument");
        aws_iam_policy::check(text, Doc::Trust).map_err(|e| malformed(e.0))?;
        self.entity_mut(Kind::Role, r.s("RoleName"))?.trust = Some(text.to_owned());
        Ok(Value::Null)
    }

    /// `UpdateUser` and `UpdateGroup`: a new path, a new name. A rename moves the record and everything that names it.
    pub(crate) fn update_named(&mut self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        let old = r.s(&name_key(kind)).to_owned();
        let mut e = self.entity(kind, &old)?.clone();
        let new_name = r
            .opt(&format!("New{}Name", kind.title()))
            .map(str::to_owned);
        if let Some(n) = &new_name
            && *n != old
            && self.entities.get(&entity_key(kind, n)).is_some()
        {
            return Err(exists(kind.title(), n));
        }
        if let Some(p) = r.opt("NewPath") {
            e.path = p.to_owned();
        }
        if let Some(n) = new_name {
            e.name = n;
        }
        e.arn = arn_of(r.account(), kind.word(), &e.path, &e.name);
        if e.name != old {
            self.renamed(kind, &old, &e.name);
        }
        self.entities.remove(&entity_key(kind, &old));
        self.entities.insert(entity_key(kind, &e.name), e);
        Ok(Value::Null)
    }

    /// Everything that held the old name of a user or group holds the new.
    fn renamed(&mut self, kind: Kind, old: &str, new: &str) {
        match kind {
            Kind::User => {
                let ids: Vec<String> = self
                    .keys
                    .iter()
                    .filter(|(_, k)| k.user == old)
                    .map(|(id, _)| id.to_owned())
                    .collect();
                for id in ids {
                    if let Some(k) = self.keys.get_mut(&id) {
                        k.user = new.to_owned();
                    }
                }
            }
            Kind::Group => {
                let users: Vec<String> = self
                    .entities
                    .with_prefix("user/")
                    .filter(|(_, u)| u.groups.contains(old))
                    .map(|(k, _)| k.to_owned())
                    .collect();
                for key in users {
                    if let Some(u) = self.entities.get_mut(&key) {
                        u.groups.remove(old);
                        u.groups.insert(new.to_owned());
                    }
                }
            }
            Kind::Role => {}
        }
    }

    pub(crate) fn delete_entity(&mut self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s(&name_key(kind)).to_owned();
        let e = self.entity(kind, &name)?;
        if !e.inline.is_empty() || !e.attached.is_empty() {
            return Err(conflict(
                "Cannot delete entity, must detach all policies first.",
            ));
        }
        match kind {
            Kind::User => {
                if self.keys.iter().any(|(_, k)| k.user == name) {
                    return Err(conflict(
                        "Cannot delete entity, must delete access keys first.",
                    ));
                }
                if !e.groups.is_empty() {
                    return Err(conflict(
                        "Cannot delete entity, must remove users from groups first.",
                    ));
                }
            }
            Kind::Group => {
                if self
                    .entities
                    .with_prefix("user/")
                    .any(|(_, u)| u.groups.contains(&name))
                {
                    return Err(conflict(
                        "Cannot delete entity, must remove users from group first.",
                    ));
                }
            }
            Kind::Role => {
                if self.profiles.iter().any(|(_, p)| p.roles.contains(&name)) {
                    return Err(conflict(
                        "Cannot delete entity, must remove roles from instance profiles first.",
                    ));
                }
            }
        }
        self.entities.remove(&entity_key(kind, &name));
        Ok(Value::Null)
    }

    pub(crate) fn tag_entity(&mut self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        let new = r.tags();
        let e = self.entity_mut(kind, r.s(&name_key(kind)))?;
        let added = new.iter().filter(|(k, _)| !e.tags.contains_key(k)).count();
        if e.tags.len() + added > MAX_TAGS {
            return Err(limit("Cannot exceed quota for TagsPerEntity: 50"));
        }
        e.tags.extend(new);
        Ok(Value::Null)
    }

    pub(crate) fn untag_entity(&mut self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        let keys = r.strings("TagKeys");
        let e = self.entity_mut(kind, r.s(&name_key(kind)))?;
        for k in keys {
            e.tags.remove(&k);
        }
        Ok(Value::Null)
    }

    pub(crate) fn list_entity_tags(&self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        let e = self.entity(kind, r.s(&name_key(kind)))?;
        let Value::Array(items) = tag_list(e.tags.iter()) else {
            return Ok(Value::Null);
        };
        page(r, items, "Tags")
    }
}
