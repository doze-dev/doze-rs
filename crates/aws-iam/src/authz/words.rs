//! The words and the choice of policy for a decision: the end of AWS's sentence for a refusal, the identity policy that let a request
//! through, and the access log's line for what allowed it.

use aws_iam_policy::{Decision, Layer, Verdict};
use aws_peers::Need;

use super::Principal;
use crate::gather::Source;

/// The end of AWS's sentence for a refusal (after who, what and on what): which layer said no.
pub(super) fn why_refused(p: &Principal, need: &Need, d: &Decision) -> String {
    let action = &need.action;
    if d.verdict == Verdict::ExplicitDeny {
        let layer = match d.denied_by() {
            Some(Layer::Boundary) => "a permissions boundary",
            Some(Layer::Session) => "a session policy",
            Some(Layer::Resource) => "a resource-based policy",
            _ => "an identity-based policy",
        };
        format!("with an explicit deny in {layer}")
    } else {
        let allowed = |l: Layer| {
            d.matched
                .iter()
                .any(|m| m.layer == l && m.effect == aws_iam_policy::Effect::Allow)
        };
        let missing = if !allowed(Layer::Identity) {
            "identity-based policy"
        } else if need.gate && !allowed(Layer::Resource) {
            "resource-based policy"
        } else if !p.boundary.is_empty() && !allowed(Layer::Boundary) {
            "permissions boundary"
        } else {
            "session policy"
        };
        format!("because no {missing} allows the {action} action")
    }
}

/// The identity policy that let a request through: the first Allow statement that matched in the identity layer, in the order the
/// principal's policies are evaluated (see [`Core::policies_of`]). `None` for a refusal, and for a request only a resource-based
/// policy or a session allowed (no identity policy did).
pub(super) fn identity_policy<'a>(p: &'a Principal, d: &Decision) -> Option<&'a Source> {
    if !d.allowed() {
        return None;
    }
    let m = d
        .matched
        .iter()
        .find(|m| m.layer == Layer::Identity && m.effect == aws_iam_policy::Effect::Allow)?;
    p.identity.sources.get(m.document)
}

/// For the access log: the statement that let a request through.
pub(super) fn allowed_by(p: &Principal, d: &Decision) -> String {
    let Some(m) = d
        .matched
        .iter()
        .find(|m| m.effect == aws_iam_policy::Effect::Allow)
    else {
        return "allowed".into();
    };
    let source = match m.layer {
        Layer::Identity => p.identity.sources.get(m.document),
        Layer::Boundary => p.boundary.sources.get(m.document),
        Layer::Session => p.session.as_ref().and_then(|s| s.sources.get(m.document)),
        Layer::Resource => None,
    };
    let statement = m
        .sid
        .clone()
        .unwrap_or_else(|| format!("statement {}", m.statement + 1));
    match source {
        Some(s) => format!("allowed by {statement} of {} ({})", s.id, s.kind),
        None => format!("allowed by {statement} of the resource-based policy"),
    }
}
