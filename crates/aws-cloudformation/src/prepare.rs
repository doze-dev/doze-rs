//! What CreateStack, UpdateStack and CreateChangeSet do before anything is made: get the template (body, URL or the previous one),
//! bind the parameters, run the transforms, check the result, resolve SSM-typed parameters, check the rules, the capabilities and the
//! resource types. Anything wrong here is a 400 at the call, as on AWS.

use std::collections::BTreeMap;

use serde_json::Value;

use aws_wire::ApiError;

use crate::core::Core;
use crate::errors::{capabilities, validation};
use crate::model::Stack;
use crate::req::Req;
use crate::tmpl::eval::Pseudo;
use crate::tmpl::params::{self, Bound, Decl};
use crate::tmpl::{self, conditions, rules};

/// A template ready to provision.
#[derive(Debug, Clone)]
pub(crate) struct Prepared {
    pub(crate) body: String,
    pub(crate) processed: Value,
    pub(crate) params: Vec<Bound>,
    pub(crate) description: Option<String>,
}

/// How strict the capability check is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Caps {
    /// CreateStack and UpdateStack: a transform needs `CAPABILITY_AUTO_EXPAND`.
    Stack,
    /// CreateChangeSet: transforms are expanded into the change set.
    ChangeSet,
}

/// The values `Ref` gives for each parameter.
pub(crate) fn ref_values(t: &Value, bound: &[Bound]) -> BTreeMap<String, Value> {
    let decls = params::declarations(t).unwrap_or_default();
    decls
        .iter()
        .filter_map(|d| {
            bound
                .iter()
                .find(|b| b.key == d.key)
                .map(|b| (d.key.clone(), params::ref_value(d, b)))
        })
        .collect()
}

impl Core {
    /// The template the request names.
    pub(crate) async fn template_text(
        &self,
        r: &Req<'_>,
        previous: Option<&Stack>,
    ) -> Result<String, ApiError> {
        let body = r.opt("TemplateBody");
        let url = r.opt("TemplateURL");
        if r.flag("UsePreviousTemplate") == Some(true) {
            if body.is_some() || url.is_some() {
                // (memory)
                return Err(validation(
                    "You cannot specify both usePreviousTemplate and Template Body/Template URL",
                ));
            }
            return previous.map(|s| s.body.clone()).ok_or_else(|| {
                validation("Either Template URL or Template Body must be specified.")
            });
        }
        match (body, url) {
            (Some(b), None) => Ok(b.to_owned()),
            (None, Some(u)) => {
                crate::transform::include::fetch(self.peers.as_deref(), &r.call.caller, u).await
            }
            // (memory)
            (Some(_), Some(_)) => Err(validation(
                "Specify exactly one of TemplateBody or TemplateUrl.",
            )),
            // Recorded 2026-11-12.
            (None, None) => Err(validation(
                "Either Template URL or Template Body must be specified.",
            )),
        }
    }

    /// Everything but the provisioning.
    pub(crate) async fn prepare(
        &self,
        r: &Req<'_>,
        body: String,
        previous: &[Bound],
        pseudo: &Pseudo,
        caps: Caps,
    ) -> Result<Prepared, ApiError> {
        let original = tmpl::read(&body)?;
        let transforms = tmpl::transforms(&original);
        let decls = params::declarations(&original)?;
        let mut bound = params::bind(&decls, &r.params(), previous)?;
        let values = ref_values(&original, &bound);
        let processed =
            crate::transform::apply(&original, self, &r.call.caller, &values, pseudo).await?;
        tmpl::check(&processed)?;
        let decls = params::declarations(&processed)?;
        // SAM may declare parameters of its own; bind again against what the template now declares.
        if decls.len() != bound.len() {
            bound = params::bind(&decls, &r.params(), previous)?;
        }
        self.resolve_ssm(r, &decls, &mut bound).await?;
        let values = ref_values(&processed, &bound);
        rules::check(&processed, &values, pseudo)?;
        let conds = conditions::evaluate(&processed, &values, pseudo)?;
        let all = tmpl::resources(&processed)
            .map(|r| r.keys().cloned().collect())
            .unwrap_or_default();
        tmpl::deps::order(&tmpl::deps::graph(&processed, &all, &conds))?;
        let given = r.strings("Capabilities");
        let mut needed: Vec<&str> = Vec::new();
        if let Some((cap, _)) = tmpl::capabilities(&processed)
            && !given
                .iter()
                .any(|g| g == cap || (cap == "CAPABILITY_IAM" && g == "CAPABILITY_NAMED_IAM"))
        {
            needed.push(cap);
        }
        if caps == Caps::Stack
            && transforms.iter().any(|t| t != "AWS::Include")
            && !given.iter().any(|g| g == "CAPABILITY_AUTO_EXPAND")
        {
            needed.push("CAPABILITY_AUTO_EXPAND");
        }
        if !needed.is_empty() {
            return Err(capabilities(&needed));
        }
        crate::res::policy::check(&processed, &self.config)?;
        Ok(Prepared {
            body,
            description: processed
                .get("Description")
                .and_then(Value::as_str)
                .map(str::to_owned),
            processed,
            params: bound,
        })
    }

    /// Reads the SSM parameters that SSM-typed template parameters name.
    async fn resolve_ssm(
        &self,
        r: &Req<'_>,
        decls: &[Decl],
        bound: &mut [Bound],
    ) -> Result<(), ApiError> {
        let mut missing = Vec::new();
        for (d, b) in decls.iter().zip(bound.iter_mut()) {
            if !d.reads_ssm() {
                continue;
            }
            match crate::dynref::ssm_value(self.peers.as_deref(), &r.call.caller, &b.value, false)
                .await
            {
                Ok(v) => b.resolved = Some(v),
                Err(_) => missing.push(b.value.clone()),
            }
        }
        if !missing.is_empty() {
            // (memory)
            return Err(validation(format!(
                "Unable to fetch parameters [{}] from parameter store for this account.",
                missing.join(",")
            )));
        }
        Ok(())
    }
}
