//! Code signing configurations: accepted and stored, never enforced (a package is not checked against a signing profile locally).

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::config::stamp;
use crate::core::{Core, Out};
use crate::errors::{invalid, not_found};
use crate::functions::{page, uuid};
use crate::model::Signing;
use crate::req::Req;

fn view(s: &Signing) -> Value {
    let mut cfg: Map<String, Value> = serde_json::from_str(&s.config).unwrap_or_default();
    cfg.insert("CodeSigningConfigId".into(), json!(s.id));
    cfg.insert("CodeSigningConfigArn".into(), json!(s.arn));
    cfg.insert("LastModified".into(), json!(s.modified));
    cfg.entry("Description").or_insert(json!(""));
    cfg.entry("CodeSigningPolicies").or_insert(json!({"UntrustedArtifactOnDeployment": "Warn"}));
    Value::Object(cfg)
}

fn members(r: &Req) -> String {
    let mut cfg = Map::new();
    for k in ["Description", "AllowedPublishers", "CodeSigningPolicies"] {
        if let Some(v) = r.value(k) {
            cfg.insert(k.into(), v.clone());
        }
    }
    Value::Object(cfg).to_string()
}

impl Core {
    pub(crate) fn create_signing(&mut self, r: &Req) -> Result<Out, ApiError> {
        let id = format!("csc-{}", &uuid()?.replace('-', "")[..17]);
        let arn = format!("arn:aws:lambda:{}:{}:code-signing-config:{id}", r.region(), r.account());
        let s = Signing { arn: arn.clone(), id, config: members(r), modified: stamp(self.now_ms()) };
        let out = json!({"CodeSigningConfig": view(&s)});
        self.signing.insert(arn, s);
        Ok(Out { json: out, status: Some(201) })
    }

    fn signing_of(&self, arn: &str) -> Result<&Signing, ApiError> {
        self.signing
            .get(arn)
            .ok_or_else(|| not_found(format!("The code signing configuration cannot be found. Check that the provided configuration is not deleted: {arn}.")))
    }

    pub(crate) fn get_signing(&self, r: &Req) -> Result<Out, ApiError> {
        Ok(json!({"CodeSigningConfig": view(self.signing_of(r.text("CodeSigningConfigArn"))?)}).into())
    }

    pub(crate) fn update_signing(&mut self, r: &Req) -> Result<Out, ApiError> {
        let arn = r.text("CodeSigningConfigArn").to_owned();
        let mut s = self.signing_of(&arn)?.clone();
        let mut cfg: Map<String, Value> = serde_json::from_str(&s.config).unwrap_or_default();
        let given: Map<String, Value> = serde_json::from_str(&members(r)).unwrap_or_default();
        cfg.extend(given);
        s.config = Value::Object(cfg).to_string();
        s.modified = stamp(self.now_ms());
        let out = json!({"CodeSigningConfig": view(&s)});
        self.signing.insert(arn, s);
        Ok(out.into())
    }

    pub(crate) fn delete_signing(&mut self, r: &Req) -> Result<Out, ApiError> {
        let arn = r.text("CodeSigningConfigArn");
        self.signing_of(arn)?;
        if self.functions.iter().any(|(_, f)| f.extras.get("CodeSigningConfigArn").is_some_and(|a| a == arn)) {
            return Err(crate::errors::in_use("The code signing configuration is in use by a function."));
        }
        self.signing.remove(arn);
        Ok(Out { json: json!({}), status: Some(204) })
    }

    pub(crate) fn list_signing(&self, r: &Req) -> Result<Out, ApiError> {
        let mut rows: Vec<Value> = self.signing.iter().map(|(_, s)| view(s)).collect();
        rows.sort_by_key(|v| v.get("CodeSigningConfigArn").and_then(Value::as_str).map(str::to_owned));
        Ok(page(r, rows, "CodeSigningConfigs", |v| {
            v.get("CodeSigningConfigArn").and_then(Value::as_str).unwrap_or_default().to_owned()
        })
        .into())
    }

    pub(crate) fn put_function_signing(&mut self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        self.find(r, &f)?;
        let arn = r.text("CodeSigningConfigArn");
        self.signing_of(arn)?;
        self.find_mut(r, &f)?.extras.insert("CodeSigningConfigArn".into(), arn.to_owned());
        Ok(json!({"CodeSigningConfigArn": arn, "FunctionName": f.name}).into())
    }

    pub(crate) fn get_function_signing(&self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        Ok(json!({"CodeSigningConfigArn": function.extras.get("CodeSigningConfigArn").cloned().unwrap_or_default(),
            "FunctionName": f.name})
        .into())
    }

    pub(crate) fn delete_function_signing(&mut self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        self.find_mut(r, &f)?.extras.remove("CodeSigningConfigArn");
        Ok(Out { json: json!({}), status: Some(204) })
    }

    pub(crate) fn functions_by_signing(&self, r: &Req) -> Result<Out, ApiError> {
        let arn = r.text("CodeSigningConfigArn");
        self.signing_of(arn).map_err(|_| invalid("The code signing configuration does not exist."))?;
        let mut names: Vec<Value> = self
            .functions
            .iter()
            .filter(|(_, f)| f.extras.get("CodeSigningConfigArn").is_some_and(|a| a == arn))
            .map(|(n, _)| json!(self.arn(r, n, None)))
            .collect();
        names.sort_by_key(|v| v.as_str().map(str::to_owned));
        Ok(json!({"FunctionArns": names}).into())
    }
}
