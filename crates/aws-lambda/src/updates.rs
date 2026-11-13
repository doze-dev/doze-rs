//! Changing a function: publishing a version, replacing its code, changing its configuration.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::config;
use crate::core::{Core, Out, Staged};
use crate::errors::{invalid, precondition};
use crate::functions::{LATEST, uuid};
use crate::model::{Function, Published, Snap};
use crate::req::Req;

impl Core {
    /// Publishes the current `$LATEST` of `function` as a new version, or answers the version that already holds the same
    /// configuration and code.
    pub(crate) fn publish(
        function: &mut Function,
        description: Option<&str>,
        now: i64,
    ) -> Result<u32, ApiError> {
        let mut cfg: Map<String, Value> =
            serde_json::from_str(&function.latest.config).unwrap_or_default();
        if let Some(d) = description {
            cfg.insert("Description".into(), json!(d));
        }
        let text = Value::Object(cfg.clone()).to_string();
        // Whether anything changed is judged without the description, which each version is given on its own.
        let comparable = |config: &str| {
            let mut m: Map<String, Value> = serde_json::from_str(config).unwrap_or_default();
            m.remove("Description");
            Value::Object(m).to_string()
        };
        if let Some(last) = function.versions.last()
            && comparable(&last.snap.config) == comparable(&text)
            && last.snap.code.sha256 == function.latest.code.sha256
            && last.snap.code.image == function.latest.code.image
        {
            return Ok(last.number);
        }
        let number = function.next_version;
        function.next_version += 1;
        function.versions.push(Published {
            number,
            snap: Snap {
                config: text,
                code: function.latest.code.clone(),
                revision: uuid()?,
                modified_ms: now,
            },
        });
        Ok(number)
    }

    pub(crate) fn publish_version(&mut self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        if let Some(want) = r.opt("CodeSha256")
            && want != function.latest.code.sha256
        {
            return Err(invalid(format!(
                "CodeSHA256 ({want}) is different from current CodeSHA256 in $LATEST ({}). Please try again with the CodeSHA256 in $LATEST.",
                function.latest.code.sha256
            )));
        }
        if let Some(rev) = r.opt("RevisionId")
            && rev != function.latest.revision
        {
            return Err(precondition(
                "The Revision Id provided does not match the latest Revision Id. Call the GetFunction/GetAlias API to retrieve the latest Revision Id",
            ));
        }
        let now = self.now_ms();
        let local = function.latest.code.local.is_some();
        let dir = self.dir.clone();
        let function = self.find_mut(r, &f)?;
        let number = Self::publish(function, r.opt("Description"), now)?;
        if local && let Some(v) = function.versions.iter_mut().find(|v| v.number == number) {
            // A version freezes code run in place: a copy, so the directory can go on changing under `$LATEST`.
            v.snap.code = crate::code::freeze(&dir, &v.snap.code)?;
        }
        let function = self.find(r, &f)?;
        let snap = function
            .versions
            .iter()
            .find(|v| v.number == number)
            .map(|v| &v.snap);
        let shown = snap.map_or(Value::Null, |s| {
            self.shown(r, &f.name, s, &number.to_string(), None)
        });
        Ok(Out {
            json: shown,
            status: Some(201),
        })
    }

    pub(crate) fn update_function_code(
        &mut self,
        r: &Req,
        staged: &Staged,
    ) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        if f.qualifier.as_deref().is_some_and(|q| q != LATEST) {
            return Err(invalid(
                "This API only supports unqualified ARN or $LATEST.",
            ));
        }
        if let Some(rev) = r.opt("RevisionId")
            && rev != function.latest.revision
        {
            return Err(precondition(
                "The Revision Id provided does not match the latest Revision Id. Call the GetFunction/GetAlias API to retrieve the latest Revision Id",
            ));
        }
        let code = staged
            .code
            .clone()
            .ok_or_else(|| invalid("Please provide a source for function code."))?;
        let now = self.now_ms();
        let publish = r.flag("Publish").unwrap_or(false) && r.flag("DryRun") != Some(true);
        if r.flag("DryRun") == Some(true) {
            return Ok(self
                .shown(r, &f.name, &self.find(r, &f)?.latest, LATEST, None)
                .into());
        }
        let revision = uuid()?;
        let function = self.find_mut(r, &f)?;
        function.latest.code = code;
        function.latest.revision = revision;
        function.latest.modified_ms = now;
        if let Some(archs) = r.value("Architectures") {
            let mut cfg: Map<String, Value> =
                serde_json::from_str(&function.latest.config).unwrap_or_default();
            cfg.insert("Architectures".into(), archs.clone());
            function.latest.config = Value::Object(cfg).to_string();
        }
        let version = if publish {
            Some(Self::publish(function, None, now)?)
        } else {
            None
        };
        self.sweep_code();
        let function = self.find(r, &f)?;
        let shown = match version.and_then(|n| function.versions.iter().find(|v| v.number == n)) {
            Some(v) => self.shown(r, &f.name, &v.snap, &v.number.to_string(), None),
            None => self.shown(r, &f.name, &function.latest, LATEST, None),
        };
        Ok(shown.into())
    }

    pub(crate) fn update_function_configuration(
        &mut self,
        r: &Req,
        staged: &Staged,
    ) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        if f.qualifier.as_deref().is_some_and(|q| q != LATEST) {
            return Err(invalid(
                "This API only supports unqualified ARN or $LATEST.",
            ));
        }
        if let Some(rev) = r.opt("RevisionId")
            && rev != function.latest.revision
        {
            return Err(precondition(
                "The Revision Id provided does not match the latest Revision Id. Call the GetFunction/GetAlias API to retrieve the latest Revision Id",
            ));
        }
        self.check_role(staged)?;
        let mut cfg: Map<String, Value> =
            serde_json::from_str(&function.latest.config).unwrap_or_default();
        config::merge(&mut cfg, r);
        config::check(&cfg)?;
        self.check_layers(r, &cfg)?;
        let (now, revision) = (self.now_ms(), uuid()?);
        let function = self.find_mut(r, &f)?;
        function.latest.config = Value::Object(cfg).to_string();
        function.latest.revision = revision;
        function.latest.modified_ms = now;
        let function = self.find(r, &f)?;
        let shown = self.shown(r, &f.name, &function.latest, LATEST, None);
        let settled = uuid()?;
        self.find_mut(r, &f)?.latest.revision = settled;
        Ok(shown.into())
    }
}
