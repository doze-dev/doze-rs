//! Policy evaluation, same account: the algorithm IAM documents (reference_policies_evaluation-logic), with every decision traced.
//!
//! 1. An explicit `Deny` in any layer (identity, boundary, session, resource) ends it: **explicit deny**.
//! 2. A resource policy that allows the caller *itself* (its user or session ARN, or `*`) is a **direct allow**: the boundary and the
//!    session policies do not limit it, and no identity policy is needed.
//! 3. Otherwise the identity policies must allow, and so must the boundary and the session policies when the caller has them. A resource
//!    policy that allows only the account root delegates to the identity policies and adds nothing of its own.
//! 4. Anything else is an **implicit deny**.
//!
//! Cross-account access, service control policies and resource-control policies are not modelled: the emulator has one account.

mod cond;
mod context;
mod glob;
mod vars;

pub use context::Context;
pub use vars::substitute;

use crate::document::{Document, Effect, Matcher, PrincipalKind, Principals, Statement};

/// Where a statement came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Identity,
    Boundary,
    Session,
    Resource,
}

/// How a request came out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    ExplicitDeny,
    ImplicitDeny,
}

/// A statement that applied to the request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matched {
    pub layer: Layer,
    /// Which document of the layer, in the order given.
    pub document: usize,
    /// Which statement of the document.
    pub statement: usize,
    pub sid: Option<String>,
    pub effect: Effect,
}

/// The verdict and what it rested on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub verdict: Verdict,
    /// Every statement whose action, resource, principal and conditions all matched.
    pub matched: Vec<Matched>,
    /// Condition keys that AWS supplies and this emulator does not, which a statement hung on (so the answer is a guess).
    pub unsupplied: Vec<String>,
}

impl Decision {
    #[must_use]
    pub fn allowed(&self) -> bool {
        self.verdict == Verdict::Allow
    }

    /// The layer an implicit or explicit denial is blamed on, for the words of the refusal.
    #[must_use]
    pub fn denied_by(&self) -> Option<Layer> {
        self.matched
            .iter()
            .find(|m| m.effect == Effect::Deny)
            .map(|m| m.layer)
    }
}

/// Who is asking, for a resource policy's `Principal`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Subject {
    /// Every ARN that names the caller: a user's; a role session's and the role's.
    pub arns: Vec<String>,
    pub account: String,
    /// The AWS service acting for the caller, when one is.
    pub service: Option<String>,
}

/// The documents that apply, layer by layer. `None` is a layer the caller does not have (no boundary, no session policy).
#[derive(Clone, Copy, Debug, Default)]
pub struct Policies<'a> {
    /// The caller's own policies and those of its groups.
    pub identity: &'a [&'a Document],
    pub boundary: Option<&'a [&'a Document]>,
    pub session: Option<&'a [&'a Document]>,
    /// The policy on the resource, when it has one.
    pub resource: Option<&'a [&'a Document]>,
}

/// One request.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    /// `sqs:SendMessage`.
    pub action: &'a str,
    /// The ARN the action is on (`*` for one that is on no resource).
    pub resource: &'a str,
    pub context: &'a Context,
    pub subject: &'a Subject,
}

/// Decides `request` against `policies`.
#[must_use]
pub fn evaluate(policies: &Policies<'_>, request: &Request<'_>) -> Decision {
    let mut found = Found::default();
    let identity = found.layer(Layer::Identity, policies.identity, request);
    let boundary = policies
        .boundary
        .map(|d| found.layer(Layer::Boundary, d, request));
    let session = policies
        .session
        .map(|d| found.layer(Layer::Session, d, request));
    let resource = policies
        .resource
        .map(|d| found.layer(Layer::Resource, d, request));
    let granted = resource.is_some_and(|r| r.direct)
        || (identity.allow && boundary.is_none_or(|b| b.allow) && session.is_none_or(|s| s.allow));
    let verdict = if found.denied {
        Verdict::ExplicitDeny
    } else if granted {
        Verdict::Allow
    } else {
        Verdict::ImplicitDeny
    };
    Decision {
        verdict,
        matched: found.matched,
        unsupplied: found.unsupplied,
    }
}

/// What one layer said.
struct Said {
    /// Some statement allowed.
    allow: bool,
    /// Some allowing statement names the caller itself (a resource policy only).
    direct: bool,
}

#[derive(Default)]
struct Found {
    matched: Vec<Matched>,
    unsupplied: Vec<String>,
    denied: bool,
}

impl Found {
    fn layer(&mut self, layer: Layer, docs: &[&Document], request: &Request<'_>) -> Said {
        let mut said = Said {
            allow: false,
            direct: false,
        };
        for (d, doc) in docs.iter().enumerate() {
            for (s, statement) in doc.statements.iter().enumerate() {
                if !self.applies(layer, statement, request) {
                    continue;
                }
                self.matched.push(Matched {
                    layer,
                    document: d,
                    statement: s,
                    sid: statement.sid.clone(),
                    effect: statement.effect,
                });
                match statement.effect {
                    Effect::Deny => self.denied = true,
                    Effect::Allow => {
                        said.allow = true;
                        said.direct |= layer == Layer::Resource
                            && statement
                                .principal
                                .as_ref()
                                .is_some_and(|p| names_caller(p, request.subject));
                    }
                }
            }
        }
        said
    }

    /// Whether the statement is about this request: action, resource, principal (in a resource policy) and every condition.
    fn applies(&mut self, layer: Layer, st: &Statement, request: &Request<'_>) -> bool {
        if !matches(st.actions.as_ref(), request.action, true, request.context) {
            return false;
        }
        // A statement with no resource matches any (a trust or resource policy may name the resource by where it is attached).
        if !matches(
            st.resources.as_ref(),
            request.resource,
            false,
            request.context,
        ) {
            return false;
        }
        if layer == Layer::Resource {
            let named = st
                .principal
                .as_ref()
                .is_none_or(|p| principal_matches(p, request.subject));
            if !named {
                return false;
            }
        }
        let mut met = true;
        for c in &st.conditions {
            let out = cond::holds(c, request.context);
            if !out.met {
                met = false;
                if let Some(key) = out.unsupplied
                    && !self.unsupplied.contains(&key)
                {
                    self.unsupplied.push(key);
                }
            }
        }
        met
    }
}

/// Whether `value` is one of the matcher's patterns (or none of them, for the `Not` forms). No matcher matches anything.
fn matches(m: Option<&Matcher>, value: &str, ignore_case: bool, ctx: &Context) -> bool {
    let Some(m) = m else { return true };
    let hit = m
        .patterns
        .iter()
        .any(|p| vars::substitute(p, ctx).is_some_and(|p| glob::matches(&p, value, ignore_case)));
    hit != m.not
}

/// Whether the `Principal` / `NotPrincipal` of a resource policy covers the caller.
fn principal_matches(p: &Principals, who: &Subject) -> bool {
    let hit = p.entries.iter().any(|e| entry_covers(e.kind, &e.id, who));
    hit != p.not
}

/// Whether the policy grants to the caller itself rather than to its account: the caller's own ARN, or anyone.
fn names_caller(p: &Principals, who: &Subject) -> bool {
    !p.not
        && p.entries.iter().any(|e| match e.kind {
            PrincipalKind::Anyone => true,
            PrincipalKind::Aws => e.id == "*" || who.arns.contains(&e.id),
            PrincipalKind::Service => who.service.as_deref() == Some(e.id.as_str()),
            _ => false,
        })
}

fn entry_covers(kind: PrincipalKind, id: &str, who: &Subject) -> bool {
    match kind {
        PrincipalKind::Anyone => true,
        PrincipalKind::Aws => {
            id == "*"
                || id == who.account
                || id == format!("arn:aws:iam::{}:root", who.account)
                || who.arns.iter().any(|a| a == id)
        }
        PrincipalKind::Service => who.service.as_deref() == Some(id),
        PrincipalKind::Federated | PrincipalKind::CanonicalUser => false,
    }
}

#[cfg(test)]
mod tests;
