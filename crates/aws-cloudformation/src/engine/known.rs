//! What a job knows while it runs: the template, the parameters, the conditions, and the resources as they are now.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::model::Res;
use crate::tmpl::eval::{Fault, Known, Lookup, Pseudo, Scope};
use crate::tmpl::params::Bound;
use crate::tmpl::{conditions, deps, resources};

/// A job's view of its stack.
#[derive(Debug, Clone)]
pub(crate) struct Working {
    pub(crate) template: Value,
    pub(crate) params: BTreeMap<String, Value>,
    pub(crate) pseudo: Pseudo,
    pub(crate) conditions: BTreeMap<String, bool>,
    /// The resources whose condition holds.
    pub(crate) live: BTreeSet<String>,
    pub(crate) resources: BTreeMap<String, Res>,
    pub(crate) exports: BTreeMap<String, String>,
}

impl Working {
    /// The view of `template` with `bound` parameters and the resources as they are.
    ///
    /// # Errors
    /// A condition that cannot be worked out.
    pub(crate) fn new(
        template: Value,
        bound: &[Bound],
        pseudo: Pseudo,
        exports: BTreeMap<String, String>,
        resources_now: BTreeMap<String, Res>,
    ) -> Result<Self, String> {
        let params = crate::prepare::ref_values(&template, bound);
        let conditions =
            conditions::evaluate(&template, &params, &pseudo).map_err(|e| e.message)?;
        let live = resources(&template)
            .map(|r| {
                r.iter()
                    .filter(|(_, v)| {
                        v.get("Condition")
                            .and_then(Value::as_str)
                            .is_none_or(|c| conditions.get(c).copied().unwrap_or(false))
                    })
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            template,
            params,
            pseudo,
            conditions,
            live,
            resources: resources_now,
            exports,
        })
    }

    /// The scope expressions are evaluated in.
    pub(crate) fn scope(&self) -> Scope<'_> {
        Scope {
            params: &self.params,
            pseudo: &self.pseudo,
            conditions: &self.conditions,
            mappings: self.template.get("Mappings"),
            known: self,
            exports: &self.exports,
        }
    }

    /// A resource's declaration in the template.
    pub(crate) fn declared(&self, logical: &str) -> Option<&Value> {
        resources(&self.template).and_then(|r| r.get(logical))
    }

    /// A resource's type in the template.
    pub(crate) fn type_of(&self, logical: &str) -> String {
        self.declared(logical)
            .map(crate::tmpl::type_of)
            .unwrap_or_default()
            .to_owned()
    }

    /// A resource's properties, resolved.
    ///
    /// # Errors
    /// A reference that cannot be resolved.
    pub(crate) fn props(&self, logical: &str) -> Result<Value, Fault> {
        match self.declared(logical).and_then(|r| r.get("Properties")) {
            Some(p) => Ok(self
                .scope()
                .eval(p)?
                .unwrap_or_else(|| Value::Object(Map::new()))),
            None => Ok(Value::Object(Map::new())),
        }
    }

    /// A resource attribute of the template, as text (`DeletionPolicy`, `UpdateReplacePolicy`).
    pub(crate) fn attribute_text(&self, logical: &str, key: &str) -> Option<String> {
        self.declared(logical)?
            .get(key)?
            .as_str()
            .map(str::to_owned)
    }

    /// What each live resource waits for.
    pub(crate) fn graph(&self) -> BTreeMap<String, BTreeSet<String>> {
        deps::graph(&self.template, &self.live, &self.conditions)
    }
}

impl Known for Working {
    fn reference(&self, logical: &str) -> Lookup<Value> {
        match self.resources.get(logical).and_then(|r| r.physical.clone()) {
            Some(p)
                if !self
                    .resources
                    .get(logical)
                    .is_some_and(|r| r.status == "CREATE_FAILED") =>
            {
                Lookup::Found(Value::String(p))
            }
            _ if self.live.contains(logical) => Lookup::Pending,
            _ => Lookup::Absent,
        }
    }

    fn attribute(&self, logical: &str, attr: &str) -> Lookup<Value> {
        let Some(r) = self.resources.get(logical).filter(|r| r.physical.is_some()) else {
            return if self.live.contains(logical) {
                Lookup::Pending
            } else {
                Lookup::Absent
            };
        };
        if let Some(v) = r.attrs_map().get(attr) {
            return Lookup::Found(v.clone());
        }
        if r.skipped {
            return Lookup::Found(crate::res::skip::attribute(
                &r.ty,
                logical,
                r.physical.as_deref().unwrap_or_default(),
                attr,
                &self.pseudo,
            ));
        }
        Lookup::Missing
    }
}
