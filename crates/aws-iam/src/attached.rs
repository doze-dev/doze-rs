//! What is attached to a user, group or role: inline policies (kept in the entity), managed policies (named by ARN), and the
//! permissions boundary.

use aws_iam_policy::Kind as Doc;
use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::errors::{limit, malformed, no_such, no_such_policy};
use crate::model::Kind;
use crate::req::{Req, encoded, page};

const MAX_ATTACHED: usize = 10;

fn name_key(kind: Kind) -> String {
    format!("{}Name", kind.title())
}

fn no_such_inline(kind: Kind, name: &str) -> ApiError {
    no_such(&format!("{} policy", kind.word()), name)
}

impl Core {
    pub(crate) fn put_inline(&mut self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        self.entity(kind, r.s(&name_key(kind)))?;
        let text = r.s("PolicyDocument");
        aws_iam_policy::check(text, Doc::Identity).map_err(|e| malformed(e.0))?;
        self.entity_mut(kind, r.s(&name_key(kind)))?
            .inline
            .insert(r.s("PolicyName").to_owned(), text.to_owned());
        Ok(Value::Null)
    }

    pub(crate) fn get_inline(&self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        let e = self.entity(kind, r.s(&name_key(kind)))?;
        let doc = e
            .inline
            .get(r.s("PolicyName"))
            .ok_or_else(|| no_such_inline(kind, r.s("PolicyName")))?;
        Ok(
            json!({name_key(kind): e.name, "PolicyName": r.s("PolicyName"), "PolicyDocument": encoded(doc)}),
        )
    }

    pub(crate) fn delete_inline(&mut self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        let e = self.entity_mut(kind, r.s(&name_key(kind)))?;
        e.inline
            .remove(r.s("PolicyName"))
            .ok_or_else(|| no_such_inline(kind, r.s("PolicyName")))?;
        Ok(Value::Null)
    }

    pub(crate) fn list_inline(&self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        let e = self.entity(kind, r.s(&name_key(kind)))?;
        page(
            r,
            e.inline.keys().map(|n| json!(n)).collect(),
            "PolicyNames",
        )
    }

    pub(crate) fn attach(&mut self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        self.entity(kind, r.s(&name_key(kind)))?;
        let arn = r.s("PolicyArn");
        self.known_policy(arn)?;
        let e = self.entity_mut(kind, r.s(&name_key(kind)))?;
        if !e.attached.contains(arn) && e.attached.len() >= MAX_ATTACHED {
            return Err(limit("Cannot exceed quota for PoliciesPerRole: 10"));
        }
        e.attached.insert(arn.to_owned());
        Ok(Value::Null)
    }

    pub(crate) fn detach(&mut self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        let arn = r.s("PolicyArn");
        let e = self.entity_mut(kind, r.s(&name_key(kind)))?;
        if !e.attached.remove(arn) {
            return Err(no_such_policy(arn));
        }
        Ok(Value::Null)
    }

    pub(crate) fn list_attached(&self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        let e = self.entity(kind, r.s(&name_key(kind)))?;
        let prefix = r.opt("PathPrefix").unwrap_or("/");
        let items = e
            .attached
            .iter()
            .filter_map(|arn| self.policies.get(arn))
            .filter(|p| p.path.starts_with(prefix))
            .map(|p| json!({"PolicyName": p.name, "PolicyArn": p.arn}))
            .collect();
        page(r, items, "AttachedPolicies")
    }

    pub(crate) fn put_boundary(&mut self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        self.entity(kind, r.s(&name_key(kind)))?;
        let boundary = self.checked_boundary(Some(r.s("PermissionsBoundary")))?;
        self.entity_mut(kind, r.s(&name_key(kind)))?.boundary = boundary;
        Ok(Value::Null)
    }

    pub(crate) fn delete_boundary(&mut self, kind: Kind, r: &Req<'_>) -> Result<Value, ApiError> {
        self.entity_mut(kind, r.s(&name_key(kind)))?.boundary = None;
        Ok(Value::Null)
    }
}
