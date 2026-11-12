//! Aliases: a name for a version, optionally splitting traffic between two.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::{Core, Out};
use crate::errors::{conflict, invalid, no_function, not_found, precondition};
use crate::functions::{LATEST, page, uuid};
use crate::model::Alias;
use crate::req::Req;

fn shown(arn: String, a: &Alias) -> Value {
    let mut out = Map::new();
    out.insert("AliasArn".into(), json!(arn));
    out.insert("Name".into(), json!(a.name));
    out.insert("FunctionVersion".into(), json!(a.version));
    out.insert("Description".into(), json!(a.description));
    out.insert("RevisionId".into(), json!(a.revision));
    if let Some(w) = a
        .weights
        .as_deref()
        .and_then(|w| serde_json::from_str::<Value>(w).ok())
    {
        out.insert(
            "RoutingConfig".into(),
            json!({"AdditionalVersionWeights": w}),
        );
    }
    Value::Object(out)
}

fn weights_of(r: &Req) -> Result<Option<String>, ApiError> {
    let Some(w) = r
        .value("RoutingConfig")
        .and_then(|c| c.get("AdditionalVersionWeights"))
    else {
        return Ok(None);
    };
    let total: f64 = w
        .as_object()
        .map_or(0.0, |o| o.values().filter_map(Value::as_f64).sum());
    if total > 1.0 {
        return Err(invalid(
            "Additional version weights must total at most 1.0.",
        ));
    }
    Ok(w.as_object()
        .filter(|o| !o.is_empty())
        .map(|_| w.to_string()))
}

impl Core {
    /// A version a caller names must exist: `$LATEST` or a published number.
    fn require_version(&self, r: &Req, name: &str, version: &str) -> Result<(), ApiError> {
        let f = self.functions.get(name);
        let there = version == LATEST
            || f.is_some_and(|f| f.versions.iter().any(|v| v.number.to_string() == version));
        if there {
            Ok(())
        } else {
            Err(no_function(&self.arn(r, name, Some(version))))
        }
    }

    pub(crate) fn create_alias(&mut self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        self.find(r, &f)?;
        let (name, version) = (r.text("Name"), r.text("FunctionVersion"));
        self.require_version(r, &f.name, version)?;
        let arn = self.arn(r, &f.name, Some(name));
        let alias = Alias {
            name: name.to_owned(),
            version: version.to_owned(),
            description: r.opt("Description").unwrap_or_default().to_owned(),
            weights: weights_of(r)?,
            revision: uuid()?,
        };
        if let Some(w) = r
            .value("RoutingConfig")
            .and_then(|c| c.get("AdditionalVersionWeights"))
            .and_then(Value::as_object)
        {
            for v in w.keys() {
                self.require_version(r, &f.name, v)?;
            }
        }
        let function = self.find_mut(r, &f)?;
        if function.aliases.iter().any(|a| a.name == name) {
            return Err(conflict(format!("Alias already exists: {arn}")));
        }
        let out = shown(arn, &alias);
        function.aliases.push(alias);
        Ok(Out {
            json: out,
            status: Some(201),
        })
    }

    fn alias_of<'a>(&'a self, r: &Req) -> Result<(&'a Alias, String), ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let name = r.text("Name");
        let arn = self.arn(r, &f.name, Some(name));
        function
            .aliases
            .iter()
            .find(|a| a.name == name)
            .map(|a| (a, arn.clone()))
            .ok_or_else(|| not_found(format!("Cannot find alias arn: {arn}")))
    }

    pub(crate) fn get_alias(&self, r: &Req) -> Result<Out, ApiError> {
        let (alias, arn) = self.alias_of(r)?;
        Ok(shown(arn, alias).into())
    }

    pub(crate) fn update_alias(&mut self, r: &Req) -> Result<Out, ApiError> {
        let (current, arn) = self.alias_of(r)?;
        let mut next = current.clone();
        if let Some(rev) = r.opt("RevisionId")
            && rev != current.revision
        {
            return Err(precondition(
                "The Revision Id provided does not match the latest Revision Id. Call the GetFunction/GetAlias API to retrieve the latest Revision Id",
            ));
        }
        let f = r.function()?;
        if let Some(v) = r.opt("FunctionVersion") {
            self.require_version(r, &f.name, v)?;
            v.clone_into(&mut next.version);
        }
        if let Some(d) = r.opt("Description") {
            d.clone_into(&mut next.description);
        }
        if r.value("RoutingConfig").is_some() {
            next.weights = weights_of(r)?;
        }
        next.revision = uuid()?;
        let out = shown(arn, &next);
        let function = self.find_mut(r, &f)?;
        if let Some(slot) = function.aliases.iter_mut().find(|a| a.name == next.name) {
            *slot = next;
        }
        Ok(out.into())
    }

    pub(crate) fn delete_alias(&mut self, r: &Req) -> Result<Out, ApiError> {
        self.alias_of(r)?;
        let f = r.function()?;
        let name = r.text("Name").to_owned();
        let function = self.find_mut(r, &f)?;
        function.aliases.retain(|a| a.name != name);
        function.urls.retain(|u| u.qualifier != name);
        function.policies.remove(&name);
        function.event_invoke.remove(&name);
        function.provisioned.remove(&name);
        Ok(Out {
            json: json!({}),
            status: Some(204),
        })
    }

    pub(crate) fn list_aliases(&self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let version = r.opt("FunctionVersion");
        let mut rows: Vec<&Alias> = function
            .aliases
            .iter()
            .filter(|a| version.is_none_or(|v| a.version == v))
            .collect();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        let rows = rows
            .into_iter()
            .map(|a| shown(self.arn(r, &f.name, Some(&a.name)), a))
            .collect();
        Ok(page(r, rows, "Aliases", |v| {
            v.get("Name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
        .into())
    }

    /// The alias chosen for an invocation by `name` (weighted routing picks between its version and the additional one by `draw`, a
    /// number in `[0, 1)`).
    pub(crate) fn alias_version(
        &self,
        function: &crate::model::Function,
        name: &str,
        draw: f64,
    ) -> Option<String> {
        let alias = function.aliases.iter().find(|a| a.name == name)?;
        let weights: Option<Map<String, Value>> = alias
            .weights
            .as_deref()
            .and_then(|w| serde_json::from_str(w).ok());
        let mut floor = 0.0;
        for (version, w) in weights.iter().flatten() {
            floor += w.as_f64().unwrap_or(0.0);
            if draw < floor {
                return Some(version.clone());
            }
        }
        Some(alias.version.clone())
    }
}
