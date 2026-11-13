//! `GetAccountAuthorizationDetails`: every user, group, role and managed policy of the account with the policies attached to each, in
//! one answer. What an audit tool or a policy-analysis script reads to see who can do what.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::errors::invalid_input;
use crate::model::{Entity, Kind};
use crate::req::{Req, encoded, secs, tag_list};
use crate::view;

/// The sections of the answer, in the order AWS lists them.
const SECTIONS: [(&str, &str); 4] = [
    ("UserDetailList", "User"),
    ("GroupDetailList", "Group"),
    ("RoleDetailList", "Role"),
    ("Policies", "Policy"),
];

impl Core {
    pub(crate) fn account_authorization_details(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let filter = r.strings("Filter");
        let wants = |kind: &str| {
            filter.is_empty()
                || filter.iter().any(|f| match kind {
                    "Policy" => f == "LocalManagedPolicy" || f == "AWSManagedPolicy",
                    other => f == other,
                })
        };
        let policy_filter = |aws: bool| {
            filter.is_empty()
                || filter.iter().any(|f| {
                    f == if aws {
                        "AWSManagedPolicy"
                    } else {
                        "LocalManagedPolicy"
                    }
                })
        };
        let mut lists: Vec<(&str, Vec<Value>)> = SECTIONS
            .iter()
            .map(|(name, kind)| {
                (
                    (*name),
                    if wants(kind) {
                        self.details_of(kind, &policy_filter)
                    } else {
                        Vec::new()
                    },
                )
            })
            .collect();
        // One page across all four lists, as one sequence.
        let limit = r
            .int("MaxItems")
            .map_or(100, |n| usize::try_from(n).unwrap_or(100))
            .max(1);
        let start = match r.opt("Marker") {
            Some(m) => m
                .parse::<usize>()
                .map_err(|_| invalid_input("Invalid Marker."))?,
            None => 0,
        };
        let total: usize = lists.iter().map(|(_, l)| l.len()).sum();
        let mut skipped = 0;
        let mut taken = 0;
        for (_, list) in &mut lists {
            let drop_front = start.saturating_sub(skipped).min(list.len());
            skipped += list.len();
            list.drain(..drop_front);
            let room = limit.saturating_sub(taken);
            list.truncate(room);
            taken += list.len();
        }
        let more = start + taken < total;
        let mut out = Map::new();
        for (name, list) in lists {
            out.insert(name.to_owned(), Value::Array(list));
        }
        out.insert("IsTruncated".into(), Value::Bool(more));
        if more {
            out.insert("Marker".into(), Value::String((start + taken).to_string()));
        }
        Ok(Value::Object(out))
    }

    fn details_of(&self, kind: &str, policy_filter: &dyn Fn(bool) -> bool) -> Vec<Value> {
        match kind {
            "User" => self.entities_of(Kind::User),
            "Group" => self.entities_of(Kind::Group),
            "Role" => self.entities_of(Kind::Role),
            _ => self.policy_details(policy_filter),
        }
    }

    fn entities_of(&self, kind: Kind) -> Vec<Value> {
        self.entities
            .iter()
            .filter(|(_, e)| e.kind == kind)
            .map(|(_, e)| self.entity_detail(e))
            .collect()
    }

    fn entity_detail(&self, e: &Entity) -> Value {
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
            let profiles: Vec<Value> = self
                .profiles
                .iter()
                .filter(|(_, p)| p.roles.contains(&e.name))
                .map(|(_, p)| self.profile_json(p, false))
                .collect();
            m.insert("InstanceProfileList".into(), Value::Array(profiles));
        }
        let inline: Vec<Value> = e
            .inline
            .iter()
            .map(|(name, doc)| json!({"PolicyName": name, "PolicyDocument": encoded(doc)}))
            .collect();
        m.insert(format!("{title}PolicyList"), Value::Array(inline));
        if e.kind == Kind::User {
            m.insert(
                "GroupList".into(),
                json!(e.groups.iter().collect::<Vec<_>>()),
            );
        }
        let attached: Vec<Value> = e
            .attached
            .iter()
            .filter_map(|arn| self.policies.get(arn))
            .map(|p| json!({"PolicyName": p.name, "PolicyArn": p.arn}))
            .collect();
        m.insert("AttachedManagedPolicies".into(), Value::Array(attached));
        if let Some(b) = &e.boundary {
            m.insert("PermissionsBoundary".into(), view::boundary(b));
        }
        if e.kind != Kind::Group {
            m.insert("Tags".into(), tag_list(e.tags.iter()));
        }
        if e.kind == Kind::Role
            && let Some((at, region)) = &e.last_used
        {
            m.insert(
                "RoleLastUsed".into(),
                json!({"LastUsedDate": secs(*at), "Region": region}),
            );
        }
        Value::Object(m)
    }

    /// Customer managed policies, and the AWS managed ones that something is attached to (an unattached AWS policy is not in the
    /// account\'s dump). (memory)
    fn policy_details(&self, policy_filter: &dyn Fn(bool) -> bool) -> Vec<Value> {
        self.policies
            .iter()
            .map(|(_, p)| p)
            .filter(|p| policy_filter(p.aws_managed))
            .map(|p| (p, self.uses(&p.arn)))
            .filter(|(p, (attached, bounded))| !p.aws_managed || *attached + *bounded > 0)
            .map(|(p, (attached, bounded))| {
                let mut detail = view::policy(p, attached, bounded, false);
                let versions: Vec<Value> = p
                    .versions
                    .iter()
                    .map(|v| {
                        json!({
                            "Document": encoded(&v.document),
                            "VersionId": v.id,
                            "IsDefaultVersion": v.id == p.default,
                            "CreateDate": secs(v.created_ms),
                        })
                    })
                    .collect();
                if let Value::Object(m) = &mut detail {
                    m.insert("PolicyVersionList".into(), Value::Array(versions));
                }
                detail
            })
            .collect()
    }
}
