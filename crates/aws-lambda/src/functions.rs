//! Functions: create, update, read, list, delete, and publishing versions.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::arn::{self, FnRef};
use crate::config::{self, Shown};
use crate::core::{Core, Out, Staged};
use crate::errors::{conflict, invalid, no_function, precondition};
use crate::model::{Function, Published, Snap};
use crate::req::Req;

/// How a function is shown when it is the unqualified `$LATEST`.
pub(crate) const LATEST: &str = "$LATEST";

/// A uuid, for revisions and ids.
pub(crate) fn uuid() -> Result<String, ApiError> {
    aws_seal::uuid_v4().ok_or_else(|| ApiError::internal("no randomness"))
}

impl Core {
    pub(crate) fn find(&self, r: &Req, f: &FnRef) -> Result<&Function, ApiError> {
        self.functions
            .get(&f.name)
            .ok_or_else(|| no_function(&self.arn(r, &f.name, f.qualifier.as_deref())))
    }

    /// The function the request names, mutably.
    pub(crate) fn find_mut(&mut self, r: &Req, f: &FnRef) -> Result<&mut Function, ApiError> {
        let missing = no_function(&arn::function(r.region(), r.account(), &f.name, f.qualifier.as_deref()));
        self.functions.get_mut(&f.name).ok_or(missing)
    }

    /// The size of a layer version by ARN; 0 for one not kept here.
    pub(crate) fn layer_size(&self, layer_arn: &str) -> u64 {
        let Some((_, _, name, version)) = arn::split_layer(layer_arn) else { return 0 };
        self.layers
            .get(name)
            .and_then(|l| l.versions.iter().find(|v| v.number == version))
            .map_or(0, |v| v.code.size)
    }

    /// The configuration of `snap` as shown.
    pub(crate) fn shown(&self, r: &Req, name: &str, snap: &Snap, version: &str, qualifier: Option<&str>) -> Value {
        let shown_arn = match (qualifier, version) {
            (Some(q), _) => self.arn(r, name, Some(q)),
            (None, LATEST) => self.arn(r, name, None),
            (None, v) => self.arn(r, name, Some(v)),
        };
        config::render(&Shown {
            name,
            arn: shown_arn,
            version,
            snap,
            layer_sizes: &|a| self.layer_size(a),
        })
    }

    /// The snapshot a version or alias names, and the version it is: `$LATEST`, a number, or the version an alias points to.
    pub(crate) fn snap_of<'a>(&self, r: &Req, f: &'a Function, qualifier: Option<&str>) -> Result<(&'a Snap, String), ApiError> {
        let missing = || no_function(&self.arn(r, &f.name, qualifier));
        match qualifier {
            None | Some(LATEST) => Ok((&f.latest, LATEST.to_owned())),
            Some(q) => {
                let version = f.aliases.iter().find(|a| a.name == q).map_or(q, |a| a.version.as_str());
                if version == LATEST {
                    return Ok((&f.latest, LATEST.to_owned()));
                }
                f.versions
                    .iter()
                    .find(|v| v.number.to_string() == version)
                    .map(|v| (&v.snap, version.to_owned()))
                    .ok_or_else(missing)
            }
        }
    }

    pub(crate) fn create_function(&mut self, r: &Req, staged: &Staged) -> Result<Out, ApiError> {
        let f = r.function()?;
        if f.qualifier.is_some() {
            return Err(invalid("The function name cannot have a qualifier."));
        }
        if self.functions.get(&f.name).is_some() {
            return Err(conflict(format!("Function already exist: {}", f.name)));
        }
        self.check_role(staged)?;
        let code = staged
            .code
            .clone()
            .ok_or_else(|| invalid("Please provide a source for function code."))?;
        let mut cfg = config::from_request(r);
        config::check(&cfg)?;
        self.check_layers(r, &cfg)?;
        self.check_runtime(&cfg, code.image.is_some())?;
        cfg.insert("Role".into(), json!(r.text("Role")));
        let now = self.now_ms();
        let tags = tags_of(r.value("Tags"));
        let mut function = Function {
            name: f.name.clone(),
            latest: Snap { config: Value::Object(cfg).to_string(), code, revision: uuid()?, modified_ms: now },
            versions: Vec::new(),
            next_version: 1,
            aliases: Vec::new(),
            urls: Vec::new(),
            policies: BTreeMap::new(),
            event_invoke: BTreeMap::new(),
            provisioned: BTreeMap::new(),
            reserved: None,
            tags,
            extras: BTreeMap::new(),
        };
        if let Some(arn) = r.opt("CodeSigningConfigArn") {
            function.extras.insert("CodeSigningConfigArn".into(), arn.to_owned());
        }
        let publish = r.flag("Publish").unwrap_or(false);
        let version = if publish { Some(Self::publish(&mut function, None, now)?) } else { None };
        self.functions.insert(f.name.clone(), function);
        let function = self.find(r, &f)?;
        let shown = match version {
            Some(n) => {
                let v = function.versions.iter().find(|p| p.number == n).map(|p| &p.snap);
                v.map_or(Value::Null, |s| self.shown(r, &f.name, s, &n.to_string(), None))
            }
            None => self.shown(r, &f.name, &function.latest, LATEST, None),
        };
        // The answer carries the revision of the function as it was made; it becomes active under another, as on AWS.
        let settled = uuid()?;
        self.find_mut(r, &f)?.latest.revision = settled;
        Ok(Out { json: shown, status: Some(201) })
    }

    /// A role AWS would refuse (it is not there, or does not trust Lambda).
    fn check_role(&self, staged: &Staged) -> Result<(), ApiError> {
        if staged.role == Some(false) {
            return Err(invalid("The role defined for the function cannot be assumed by Lambda."));
        }
        Ok(())
    }

    /// The runtime and handler a zip function needs.
    fn check_runtime(&self, cfg: &Map<String, Value>, image: bool) -> Result<(), ApiError> {
        let package = cfg.get("PackageType").and_then(Value::as_str);
        if image || package == Some("Image") {
            return Ok(());
        }
        if cfg.get("Runtime").is_none() || cfg.get("Handler").is_none() {
            return Err(invalid("Runtime and Handler are required for functions with a Zip package type."));
        }
        Ok(())
    }

    /// The layers a function names must be layer versions that exist, or belong to another account (kept, and flagged when it runs).
    pub(crate) fn check_layers(&self, r: &Req, cfg: &Map<String, Value>) -> Result<(), ApiError> {
        let Some(layers) = cfg.get("Layers").and_then(Value::as_array) else { return Ok(()) };
        for layer in layers.iter().filter_map(Value::as_str) {
            let Some((region, account, name, version)) = arn::split_layer(layer) else {
                return Err(invalid(format!("Layer version {layer} is not a valid layer version ARN.")));
            };
            let ours = region == self.region && account == r.account();
            let found = self
                .layers
                .get(name)
                .is_some_and(|l| l.versions.iter().any(|v| v.number == version));
            if ours && !found {
                return Err(invalid(format!("Layer version {layer} does not exist.")));
            }
        }
        Ok(())
    }

    pub(crate) fn get_function(&self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let (snap, version) = self.snap_of(r, function, f.qualifier.as_deref())?;
        let mut out = Map::new();
        out.insert(
            "Configuration".into(),
            self.shown(r, &f.name, snap, &version, f.qualifier.as_deref()),
        );
        out.insert("Code".into(), self.code_location(r, snap));
        if !function.tags.is_empty() {
            out.insert("Tags".into(), json!(function.tags));
        }
        if let Some(n) = function.reserved {
            out.insert("Concurrency".into(), json!({"ReservedConcurrentExecutions": n}));
        }
        Ok(Value::Object(out).into())
    }

    /// Where the code can be fetched (a URL on this server), or the image.
    fn code_location(&self, r: &Req, snap: &Snap) -> Value {
        if let Some(image) = &snap.code.image {
            return json!({"RepositoryType": "ECR", "ImageUri": image, "ResolvedImageUri": image});
        }
        let id = if snap.code.id.is_empty() { "local" } else { &snap.code.id };
        json!({
            "RepositoryType": "S3",
            "Location": format!("{}/_aws/lambda/code/{}/{id}", r.caller.endpoint, r.region()),
        })
    }

    pub(crate) fn get_function_configuration(&self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let (snap, version) = self.snap_of(r, function, f.qualifier.as_deref())?;
        Ok(self.shown(r, &f.name, snap, &version, f.qualifier.as_deref()).into())
    }

    pub(crate) fn list_functions(&self, r: &Req) -> Result<Out, ApiError> {
        let all = r.opt("FunctionVersion") == Some("ALL");
        let mut rows: Vec<Value> = Vec::new();
        let mut names: Vec<&str> = self.functions.iter().map(|(n, _)| n).collect();
        names.sort_unstable();
        for name in names {
            let Some(f) = self.functions.get(name) else { continue };
            rows.push(self.shown(r, name, &f.latest, LATEST, None));
            if all {
                for v in &f.versions {
                    rows.push(self.shown(r, name, &v.snap, &v.number.to_string(), None));
                }
            }
        }
        Ok(page(r, rows, "Functions", |v| {
            format!(
                "{}\u{0}{}",
                v.get("FunctionName").and_then(Value::as_str).unwrap_or_default(),
                v.get("Version").and_then(Value::as_str).unwrap_or_default()
            )
        })
        .into())
    }

    pub(crate) fn list_versions(&self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let mut rows = vec![self.shown(r, &f.name, &function.latest, LATEST, None)];
        for v in &function.versions {
            rows.push(self.shown(r, &f.name, &v.snap, &v.number.to_string(), None));
        }
        Ok(page(r, rows, "Versions", |v| {
            v.get("Version").and_then(Value::as_str).unwrap_or_default().to_owned()
        })
        .into())
    }

    pub(crate) fn delete_function(&mut self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        match f.qualifier.as_deref() {
            None => {
                self.functions.remove(&f.name);
            }
            Some(LATEST) => {
                return Err(invalid("Deletion of $LATEST is not supported. To delete the function, omit the qualifier."));
            }
            Some(q) => {
                let number: u32 = q.parse().map_err(|_| {
                    invalid("Deletion of aliases is not supported using DeleteFunction. To delete an alias, use DeleteAlias API.")
                })?;
                if !function.versions.iter().any(|v| v.number == number) {
                    return Err(no_function(&self.arn(r, &f.name, Some(q))));
                }
                if function.aliases.iter().any(|a| a.version == q) {
                    return Err(conflict(format!(
                        "Unable to delete function version {q} because an alias refers to it."
                    )));
                }
                self.find_mut(r, &f)?.versions.retain(|v| v.number != number);
            }
        }
        self.sweep_code();
        Ok(Out { json: json!({}), status: Some(204) })
    }

    /// Publishes the current `$LATEST` of `function` as a new version, or answers the version that already holds the same
    /// configuration and code.
    pub(crate) fn publish(function: &mut Function, description: Option<&str>, now: i64) -> Result<u32, ApiError> {
        let mut cfg: Map<String, Value> = serde_json::from_str(&function.latest.config).unwrap_or_default();
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
            snap: Snap { config: text, code: function.latest.code.clone(), revision: uuid()?, modified_ms: now },
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
        let snap = function.versions.iter().find(|v| v.number == number).map(|v| &v.snap);
        let shown = snap.map_or(Value::Null, |s| self.shown(r, &f.name, s, &number.to_string(), None));
        Ok(Out { json: shown, status: Some(201) })
    }

    pub(crate) fn update_function_code(&mut self, r: &Req, staged: &Staged) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        if f.qualifier.as_deref().is_some_and(|q| q != LATEST) {
            return Err(invalid("This API only supports unqualified ARN or $LATEST."));
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
            return Ok(self.shown(r, &f.name, &self.find(r, &f)?.latest, LATEST, None).into());
        }
        let revision = uuid()?;
        let function = self.find_mut(r, &f)?;
        function.latest.code = code;
        function.latest.revision = revision;
        function.latest.modified_ms = now;
        if let Some(archs) = r.value("Architectures") {
            let mut cfg: Map<String, Value> = serde_json::from_str(&function.latest.config).unwrap_or_default();
            cfg.insert("Architectures".into(), archs.clone());
            function.latest.config = Value::Object(cfg).to_string();
        }
        let version = if publish { Some(Self::publish(function, None, now)?) } else { None };
        self.sweep_code();
        let function = self.find(r, &f)?;
        let shown = match version.and_then(|n| function.versions.iter().find(|v| v.number == n)) {
            Some(v) => self.shown(r, &f.name, &v.snap, &v.number.to_string(), None),
            None => self.shown(r, &f.name, &function.latest, LATEST, None),
        };
        Ok(shown.into())
    }

    pub(crate) fn update_function_configuration(&mut self, r: &Req, staged: &Staged) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        if f.qualifier.as_deref().is_some_and(|q| q != LATEST) {
            return Err(invalid("This API only supports unqualified ARN or $LATEST."));
        }
        if let Some(rev) = r.opt("RevisionId")
            && rev != function.latest.revision
        {
            return Err(precondition(
                "The Revision Id provided does not match the latest Revision Id. Call the GetFunction/GetAlias API to retrieve the latest Revision Id",
            ));
        }
        self.check_role(staged)?;
        let mut cfg: Map<String, Value> = serde_json::from_str(&function.latest.config).unwrap_or_default();
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

/// Tags given as a JSON object of strings.
pub(crate) fn tags_of(v: Option<&Value>) -> BTreeMap<String, String> {
    v.and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// A page of `rows` under the key `key`: `Marker` is the sort key of the last row sent, `MaxItems` the page size.
pub(crate) fn page(r: &Req, rows: Vec<Value>, key: &str, sort_key: impl Fn(&Value) -> String) -> Value {
    let max = usize::try_from(r.int("MaxItems").unwrap_or(50)).unwrap_or(50).clamp(1, 10_000);
    let marker = r.opt("Marker");
    let mut rest: Vec<Value> = rows
        .into_iter()
        .filter(|row| marker.is_none_or(|m| sort_key(row).as_str() > m))
        .collect();
    let more = rest.len() > max;
    rest.truncate(max);
    let next = more.then(|| rest.last().map(&sort_key)).flatten();
    let mut out = Map::new();
    out.insert(key.to_owned(), Value::Array(rest));
    if let Some(n) = next {
        out.insert("NextMarker".into(), Value::String(n));
    }
    Value::Object(out)
}
