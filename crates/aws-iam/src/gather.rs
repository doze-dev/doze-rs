//! The documents that apply to a principal: its inline and attached policies (and its groups'), and its permissions boundary, each
//! remembered with where it came from so a decision can name its sources.

use aws_iam_policy::Document;
use aws_wire::ApiError;

use crate::core::Core;
use crate::errors::{invalid_input, no_such};
use crate::model::{Entity, Kind, Managed};

/// Where a document came from, as `PolicySourceType` words it.
#[derive(Clone, Debug)]
pub(crate) struct Source {
    /// The policy's name, or `PolicyInputList.1` for one passed in the request.
    pub(crate) id: String,
    /// `user`, `group`, `role`, `aws-managed`, `user-managed`, `resource`, or `none`.
    pub(crate) kind: &'static str,
}

/// Documents in the order given, with their sources beside them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Docs {
    pub(crate) docs: Vec<Document>,
    pub(crate) sources: Vec<Source>,
}

impl Docs {
    pub(crate) fn push(&mut self, doc: Document, id: String, kind: &'static str) {
        self.docs.push(doc);
        self.sources.push(Source { id, kind });
    }

    pub(crate) fn refs(&self) -> Vec<&Document> {
        self.docs.iter().collect()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
}

/// A stored document read back. Stored documents passed the checks, so one that does not parse is dropped.
fn read(text: &str) -> Option<Document> {
    aws_iam_policy::parse(text)?.ok()
}

/// A managed policy's default version.
fn default_document(p: &Managed) -> Option<Document> {
    p.versions
        .iter()
        .find(|v| v.id == p.default)
        .and_then(|v| read(&v.document))
}

/// Request documents (`PolicyInputList`) read as `kind` documents, named `{member}.1`, `{member}.2`...
pub(crate) fn given(
    texts: &[String],
    member: &str,
    kind: aws_iam_policy::Kind,
    out: &mut Docs,
) -> Result<(), ApiError> {
    for (i, text) in texts.iter().enumerate() {
        aws_iam_policy::check(text, kind).map_err(|e| invalid_input(e.0))?;
        if let Some(doc) = read(text) {
            out.push(doc, format!("{member}.{}", i + 1), "none");
        }
    }
    Ok(())
}

impl Core {
    /// The user, group or role an IAM ARN names.
    pub(crate) fn entity_of_arn(&self, arn: &str) -> Result<&Entity, ApiError> {
        let resource = arn.splitn(6, ':').nth(5).unwrap_or_default();
        let (word, path) = resource.split_once('/').unwrap_or_default();
        let kind = match word {
            "user" => Kind::User,
            "group" => Kind::Group,
            "role" => Kind::Role,
            _ => return Err(no_such("entity", arn)),
        };
        let name = path.rsplit('/').next().unwrap_or_default();
        self.entity(kind, name)
    }

    /// The identity policies that apply to `e` (a user's include its groups'), and its permissions boundary.
    pub(crate) fn policies_of(&self, e: &Entity) -> (Docs, Docs) {
        let mut identity = Docs::default();
        self.own_policies(e, &mut identity);
        if e.kind == Kind::User {
            for group in &e.groups {
                if let Ok(g) = self.entity(Kind::Group, group) {
                    self.own_policies(g, &mut identity);
                }
            }
        }
        let mut boundary = Docs::default();
        if let Some(p) = e.boundary.as_deref().and_then(|a| self.policies.get(a))
            && let Some(doc) = default_document(p)
        {
            boundary.push(doc, p.name.clone(), managed_kind(p));
        }
        (identity, boundary)
    }

    fn own_policies(&self, e: &Entity, out: &mut Docs) {
        for (name, text) in &e.inline {
            if let Some(doc) = read(text) {
                out.push(doc, name.clone(), e.kind.word());
            }
        }
        for arn in &e.attached {
            if let Some(p) = self.policies.get(arn)
                && let Some(doc) = default_document(p)
            {
                out.push(doc, p.name.clone(), managed_kind(p));
            }
        }
    }
}

fn managed_kind(p: &Managed) -> &'static str {
    if p.aws_managed {
        "aws-managed"
    } else {
        "user-managed"
    }
}
