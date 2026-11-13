//! Group membership. A user holds the names of its groups; a group's members are found by asking the users.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::errors::limit;
use crate::model::Kind;
use crate::req::{Req, page};
use crate::view;

const MAX_GROUPS_PER_USER: usize = 10;

impl Core {
    pub(crate) fn add_user_to_group(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        self.entity(Kind::Group, r.s("GroupName"))?;
        let user = self.entity_mut(Kind::User, r.s("UserName"))?;
        if !user.groups.contains(r.s("GroupName")) && user.groups.len() >= MAX_GROUPS_PER_USER {
            return Err(limit("Cannot exceed quota for GroupsPerUser: 10"));
        }
        user.groups.insert(r.s("GroupName").to_owned());
        Ok(Value::Null)
    }

    pub(crate) fn remove_user_from_group(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        self.entity(Kind::Group, r.s("GroupName"))?;
        let user = self.entity_mut(Kind::User, r.s("UserName"))?;
        if !user.groups.remove(r.s("GroupName")) {
            return Err(crate::errors::no_such_member(
                r.s("UserName"),
                r.s("GroupName"),
            ));
        }
        Ok(Value::Null)
    }

    pub(crate) fn get_group(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let group = self.entity(Kind::Group, r.s("GroupName"))?;
        let users: Vec<Value> = self
            .entities
            .with_prefix("user/")
            .map(|(_, u)| u)
            .filter(|u| u.groups.contains(&group.name))
            .map(|u| view::entity(u, false))
            .collect();
        let mut page_ = page(r, users, "Users")?;
        if let Value::Object(m) = &mut page_ {
            m.insert("Group".into(), view::entity(group, false));
        }
        Ok(page_)
    }

    pub(crate) fn list_groups_for_user(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let user = self.entity(Kind::User, r.s("UserName"))?;
        let items = user
            .groups
            .iter()
            .filter_map(|g| self.entity(Kind::Group, g).ok())
            .map(|g| view::entity(g, false))
            .collect();
        page(r, items, "Groups")
    }

    pub(crate) fn get_user_or_caller(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        if let Some(name) = r.opt("UserName") {
            return Ok(json!({"User": view::entity(self.entity(Kind::User, name)?, true)}));
        }
        let owner = self
            .keys
            .get(&r.call.caller.access_key_id)
            .and_then(|k| self.entity(Kind::User, &k.user).ok());
        match owner {
            Some(u) => Ok(json!({"User": view::entity(u, true)})),
            None => {
                let account = r.account();
                Ok(json!({"User": {
                    "Path": "/", "UserName": "root", "UserId": account, "Arn": format!("arn:aws:iam::{account}:root"),
                    "CreateDate": crate::req::secs(0)
                }}))
            }
        }
    }
}
