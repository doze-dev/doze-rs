//! Instance profiles: a named holder of one role.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::entity::{arn_of, new_id};
use crate::errors::{conflict, exists, limit, no_such};
use crate::model::{Kind, Profile};
use crate::req::{Req, page, tag_list};
use crate::view;

impl Core {
    fn profile_named(&self, name: &str) -> Result<&Profile, ApiError> {
        self.profiles
            .get(name)
            .ok_or_else(|| no_such("instance profile", name))
    }

    /// The profile with its roles written out.
    pub(crate) fn profile_json(&self, p: &Profile, full: bool) -> Value {
        let roles = p
            .roles
            .iter()
            .filter_map(|n| self.entity(Kind::Role, n).ok())
            .map(|e| view::entity(e, false))
            .collect();
        view::profile(p, roles, full)
    }

    pub(crate) fn create_instance_profile(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("InstanceProfileName");
        if self.profiles.get(name).is_some() {
            return Err(exists("Instance Profile", name));
        }
        let path = r.opt("Path").unwrap_or("/");
        let p = Profile {
            name: name.to_owned(),
            path: path.to_owned(),
            id: new_id("AIPA")?,
            arn: arn_of(r.account(), "instance-profile", path, name),
            created_ms: self.now_ms(),
            roles: Vec::new(),
            tags: r.tags().into_iter().collect(),
        };
        let shown = self.profile_json(&p, false);
        self.profiles.insert(name.to_owned(), p);
        Ok(json!({"InstanceProfile": shown}))
    }

    pub(crate) fn get_instance_profile(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let p = self.profile_named(r.s("InstanceProfileName"))?;
        Ok(json!({"InstanceProfile": self.profile_json(p, true)}))
    }

    pub(crate) fn delete_instance_profile(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let p = self.profile_named(r.s("InstanceProfileName"))?;
        if !p.roles.is_empty() {
            return Err(conflict(
                "Cannot delete entity, must remove roles from instance profile first.",
            ));
        }
        self.profiles.remove(r.s("InstanceProfileName"));
        Ok(Value::Null)
    }

    pub(crate) fn list_instance_profiles(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let prefix = r.opt("PathPrefix").unwrap_or("/");
        let items = self
            .profiles
            .iter()
            .map(|(_, p)| p)
            .filter(|p| p.path.starts_with(prefix))
            .map(|p| self.profile_json(p, false))
            .collect();
        page(r, items, "InstanceProfiles")
    }

    pub(crate) fn list_instance_profiles_for_role(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let role = r.s("RoleName");
        self.entity(Kind::Role, role)?;
        let items = self
            .profiles
            .iter()
            .map(|(_, p)| p)
            .filter(|p| p.roles.iter().any(|n| n == role))
            .map(|p| self.profile_json(p, false))
            .collect();
        page(r, items, "InstanceProfiles")
    }

    pub(crate) fn add_role_to_instance_profile(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("InstanceProfileName");
        let p = self.profile_named(name)?;
        self.entity(Kind::Role, r.s("RoleName"))?;
        // Recorded 2026-11-09: adding a role to a profile that holds one, the same or another, is a quota error.
        if !p.roles.is_empty() {
            return Err(limit(
                "Cannot exceed quota for InstanceSessionsPerInstanceProfile: 1",
            ));
        }
        if let Some(p) = self.profiles.get_mut(name) {
            p.roles.push(r.s("RoleName").to_owned());
        }
        Ok(Value::Null)
    }

    pub(crate) fn remove_role_from_instance_profile(
        &mut self,
        r: &Req<'_>,
    ) -> Result<Value, ApiError> {
        let name = r.s("InstanceProfileName");
        self.profile_named(name)?;
        self.entity(Kind::Role, r.s("RoleName"))?;
        if let Some(p) = self.profiles.get_mut(name) {
            p.roles.retain(|n| n != r.s("RoleName"));
        }
        Ok(Value::Null)
    }

    pub(crate) fn tag_profile(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let new: BTreeMap<String, String> = r.tags().into_iter().collect();
        self.profile_named(r.s("InstanceProfileName"))?;
        if let Some(p) = self.profiles.get_mut(r.s("InstanceProfileName")) {
            p.tags.extend(new);
        }
        Ok(Value::Null)
    }

    pub(crate) fn untag_profile(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        self.profile_named(r.s("InstanceProfileName"))?;
        if let Some(p) = self.profiles.get_mut(r.s("InstanceProfileName")) {
            for k in r.strings("TagKeys") {
                p.tags.remove(&k);
            }
        }
        Ok(Value::Null)
    }

    pub(crate) fn list_profile_tags(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let p = self.profile_named(r.s("InstanceProfileName"))?;
        let Value::Array(items) = tag_list(p.tags.iter()) else {
            return Ok(Value::Null);
        };
        page(r, items, "Tags")
    }
}
