//! The policy simulator: `SimulateCustomPolicy`, `SimulatePrincipalPolicy`, and the two `GetContextKeysFor…` operations. They run
//! the same evaluator that enforcement uses, so a simulation and a refusal cannot disagree.
//!
//! Not yet checked against AWS: no scenario has recorded these (the recorder's policy omits `iam:Simulate*`), so their answers are
//! built from the model and the documentation alone, and the scoreboard says so.

use aws_iam_policy::{Context, Decision, Kind as Doc, Layer, Policies, Request, Subject, Verdict};
use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::errors::invalid_input;
use crate::gather::{Docs, given};
use crate::req::{Req, page};

impl Core {
    pub(crate) fn simulate_custom(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let mut identity = Docs::default();
        given(
            &r.strings("PolicyInputList"),
            "PolicyInputList",
            Doc::Identity,
            &mut identity,
        )?;
        let mut boundary = Docs::default();
        given(
            &r.strings("PermissionsBoundaryPolicyInputList"),
            "PermissionsBoundaryPolicyInputList",
            Doc::Identity,
            &mut boundary,
        )?;
        let caller = r.opt("CallerArn").map(str::to_owned);
        self.simulate(
            r,
            identity,
            (!boundary.is_empty()).then_some(boundary),
            caller,
        )
    }

    pub(crate) fn simulate_principal(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let source = r.s("PolicySourceArn");
        let entity = self.entity_of_arn(source)?;
        let (mut identity, mut boundary) = self.policies_of(entity);
        given(
            &r.strings("PolicyInputList"),
            "PolicyInputList",
            Doc::Identity,
            &mut identity,
        )?;
        let own = r.strings("PermissionsBoundaryPolicyInputList");
        if !own.is_empty() {
            boundary = Docs::default();
            given(
                &own,
                "PermissionsBoundaryPolicyInputList",
                Doc::Identity,
                &mut boundary,
            )?;
        }
        let caller = r.opt("CallerArn").unwrap_or(source).to_owned();
        self.simulate(
            r,
            identity,
            (!boundary.is_empty()).then_some(boundary),
            Some(caller),
        )
    }

    fn simulate(
        &self,
        r: &Req<'_>,
        identity: Docs,
        boundary: Option<Docs>,
        caller: Option<String>,
    ) -> Result<Value, ApiError> {
        let mut resource = Docs::default();
        if let Some(text) = r.opt("ResourcePolicy") {
            given(
                &[text.to_owned()],
                "ResourcePolicy",
                Doc::Resource,
                &mut resource,
            )?;
        }
        let actions = r.strings("ActionNames");
        if actions.is_empty() {
            return Err(invalid_input("ActionNames must not be empty."));
        }
        let mut resources = r.strings("ResourceArns");
        if resources.is_empty() {
            resources.push("*".to_owned());
        }
        let mut context = context_of(r);
        // Every key a policy asks about and the request did not give is one AWS has and we were not told: say so, and say which.
        let (identity_refs, resource_refs) = (identity.refs(), resource.refs());
        let boundary_refs = boundary.as_ref().map(Docs::refs);
        let all: Vec<&_> = identity_refs
            .iter()
            .chain(&resource_refs)
            .chain(boundary_refs.iter().flatten())
            .copied()
            .collect();
        for key in aws_iam_policy::context_keys(&all) {
            if context.get(&key).is_none() {
                context.leave_unsupplied(&key);
            }
        }
        let subject = Subject {
            arns: caller.into_iter().collect(),
            account: r.account().to_owned(),
            service: None,
        };
        let policies = Policies {
            identity: &identity_refs,
            boundary: boundary_refs.as_deref(),
            session: None,
            resource: (!resource.is_empty()).then_some(resource_refs.as_slice()),
        };
        let mut results = Vec::new();
        for action in &actions {
            for arn in &resources {
                let request = Request {
                    action,
                    resource: arn,
                    context: &context,
                    subject: &subject,
                };
                let decision = aws_iam_policy::evaluate(&policies, &request);
                results.push(result(
                    action,
                    arn,
                    &decision,
                    &identity,
                    boundary.as_ref(),
                    &resource,
                ));
            }
        }
        page(r, results, "EvaluationResults")
    }

    pub(crate) fn context_keys_custom(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let mut docs = Docs::default();
        given(
            &r.strings("PolicyInputList"),
            "PolicyInputList",
            Doc::Identity,
            &mut docs,
        )?;
        Ok(keys_of(&docs))
    }

    pub(crate) fn context_keys_principal(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let entity = self.entity_of_arn(r.s("PolicySourceArn"))?;
        let (mut docs, _) = self.policies_of(entity);
        given(
            &r.strings("PolicyInputList"),
            "PolicyInputList",
            Doc::Identity,
            &mut docs,
        )?;
        Ok(keys_of(&docs))
    }
}

fn keys_of(docs: &Docs) -> Value {
    json!({"ContextKeyNames": aws_iam_policy::context_keys(&docs.refs())})
}

fn context_of(r: &Req<'_>) -> Context {
    let mut context = Context::new();
    let entries = r.v.get("ContextEntries").and_then(Value::as_array);
    for e in entries.into_iter().flatten() {
        let name = e
            .get("ContextKeyName")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let values = e
            .get("ContextKeyValues")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        context.set_many(name, values);
    }
    context
}

fn decision_word(v: Verdict) -> &'static str {
    match v {
        Verdict::Allow => "ALLOWED",
        Verdict::ExplicitDeny => "EXPLICIT_DENY",
        Verdict::ImplicitDeny => "IMPLICIT_DENY",
    }
}

fn result(
    action: &str,
    resource: &str,
    d: &Decision,
    identity: &Docs,
    boundary: Option<&Docs>,
    resource_docs: &Docs,
) -> Value {
    let matched: Vec<Value> = d
        .matched
        .iter()
        .filter_map(|m| {
            let docs = match m.layer {
                Layer::Identity => Some(identity),
                Layer::Boundary => boundary,
                Layer::Resource => Some(resource_docs),
                Layer::Session => None,
            }?;
            let s = docs.sources.get(m.document)?;
            Some(json!({"SourcePolicyId": s.id, "SourcePolicyType": if m.layer == Layer::Resource { "resource" } else { s.kind }}))
        })
        .collect();
    let mut out = serde_json::Map::new();
    let base = json!({
        "EvalActionName": action,
        "EvalResourceName": resource,
        "EvalDecision": decision_word(d.verdict),
        "MatchedStatements": matched,
        "MissingContextValues": d.unsupplied,
    });
    if let Value::Object(map) = base {
        out = map;
    }
    if boundary.is_some() {
        let allowed = d
            .matched
            .iter()
            .any(|m| m.layer == Layer::Boundary && m.effect == aws_iam_policy::Effect::Allow);
        out.insert(
            "PermissionsBoundaryDecisionDetail".to_owned(),
            json!({"AllowedByPermissionsBoundary": allowed}),
        );
    }
    Value::Object(out)
}
