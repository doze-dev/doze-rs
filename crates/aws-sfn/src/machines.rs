//! State machines: create (idempotent on the same name, definition and role, as recorded), describe (the logging, tracing and
//! encryption blocks as stored or AWS's defaults, as recorded), update, delete (immediate: running executions carry their own
//! frozen definition), list, and `ValidateStateMachineDefinition`.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::arn::{self, Target};
use crate::core::Core;
use crate::model::{Machine, Version};
use crate::req::{
    Req, invalid_arn, invalid_definition, invalid_name, no_machine, put, secs, token, valid_name,
};

impl Core {
    pub(crate) async fn create_machine(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("name");
        if !valid_name(name) {
            return Err(invalid_name(name));
        }
        let role = r.s("roleArn");
        check_role(role)?;
        let express = r.opt("type") == Some("EXPRESS");
        let text = r.s("definition");
        check_definition(text, express)?;
        let logging = r.v.get("loggingConfiguration").map(Value::to_string);
        check_logging(logging.as_deref(), express)?;
        let arn = arn::machine(r.region(), r.account(), name);
        if let Some(m) = self.machines.get(&arn) {
            if m.definition == text && m.role_arn == role && m.express == express {
                return Ok(json!({"stateMachineArn": arn, "creationDate": secs(m.created_ms)}));
            }
            return Err(ApiError::modeled(
                "StateMachineAlreadyExists",
                format!("State Machine Already Exists: '{arn}'"),
            ));
        }
        crate::role::check(self.peers.as_deref(), &r.call.caller, role).await?;
        let now = self.now_ms();
        let mut m = Machine {
            arn: arn.clone(),
            name: name.to_owned(),
            express,
            definition: text.to_owned(),
            role_arn: role.to_owned(),
            description: None,
            logging,
            tracing: r.v.get("tracingConfiguration").map(Value::to_string),
            encryption: r.v.get("encryptionConfiguration").map(Value::to_string),
            created_ms: now,
            updated_ms: now,
            revision: crate::util::uuid(),
            tags: r.tags().into_iter().collect::<BTreeMap<_, _>>(),
            versions: Vec::new(),
            next_version: 1,
            aliases: Vec::new(),
            endpoint: r.call.caller.endpoint.clone(),
        };
        let mut out = json!({"stateMachineArn": arn, "creationDate": secs(now)});
        if r.flag("publish") {
            let v = publish(&mut m, r.opt("versionDescription"), now);
            out["stateMachineVersionArn"] = json!(v);
        }
        self.machines.insert(arn, m);
        Ok(out)
    }

    pub(crate) async fn update_machine(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let given = r.s("stateMachineArn");
        let (_, t) = arn::target(given).ok_or_else(|| invalid_arn("Arn", given))?;
        let Target::Machine(_) = t else {
            return Err(invalid_arn("Arn", given));
        };
        let changes = [
            "definition",
            "roleArn",
            "loggingConfiguration",
            "tracingConfiguration",
            "encryptionConfiguration",
        ];
        if !changes.iter().any(|k| r.v.get(*k).is_some()) {
            // (memory)
            return Err(ApiError::modeled(
                "MissingRequiredParameter",
                "Either the definition, the role ARN, the LoggingConfiguration, the EncryptionConfiguration or the TracingConfiguration must be specified",
            ));
        }
        let m = self.machines.get(given).ok_or_else(|| no_machine(given))?;
        let express = m.express;
        if let Some(text) = r.opt("definition") {
            check_definition(text, express)?;
        }
        if let Some(role) = r.opt("roleArn") {
            check_role(role)?;
            crate::role::check(self.peers.as_deref(), &r.call.caller, role).await?;
        }
        let logging = r.v.get("loggingConfiguration").map(Value::to_string);
        check_logging(logging.as_deref(), express)?;
        let now = self.now_ms();
        let Some(m) = self.machines.get_mut(given) else {
            return Err(no_machine(given));
        };
        if let Some(text) = r.opt("definition") {
            text.clone_into(&mut m.definition);
        }
        if let Some(role) = r.opt("roleArn") {
            role.clone_into(&mut m.role_arn);
        }
        if logging.is_some() {
            m.logging = logging;
        }
        if let Some(t) = r.v.get("tracingConfiguration") {
            m.tracing = Some(t.to_string());
        }
        if let Some(e) = r.v.get("encryptionConfiguration") {
            m.encryption = Some(e.to_string());
        }
        m.updated_ms = now;
        m.revision = crate::util::uuid();
        let mut out = json!({"updateDate": secs(now), "revisionId": m.revision});
        if r.flag("publish") {
            let v = publish(m, r.opt("versionDescription"), now);
            out["stateMachineVersionArn"] = json!(v);
        }
        Ok(out)
    }

    pub(crate) fn describe_machine(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let given = r.s("stateMachineArn");
        let (p, t) = arn::target(given).ok_or_else(|| invalid_arn("Arn", given))?;
        let key = arn::machine(p.region, p.account, t.machine());
        let m = self.machines.get(&key).ok_or_else(|| no_machine(given))?;
        match t {
            Target::Machine(_) => Ok(describe(m)),
            Target::Version(_, n) => {
                let v = m
                    .versions
                    .iter()
                    .find(|v| v.number == n)
                    .ok_or_else(|| no_machine(given))?;
                Ok(describe_version(m, v))
            }
            Target::Alias(..) => Err(invalid_arn("Arn", given)),
        }
    }

    pub(crate) fn delete_machine(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let given = r.s("stateMachineArn");
        let (_, t) = arn::target(given).ok_or_else(|| invalid_arn("Arn", given))?;
        if !matches!(t, Target::Machine(_)) {
            // (memory)
            return Err(crate::req::invalid(
                "The state machine ARN must not include a version or an alias: delete them with DeleteStateMachineVersion or DeleteStateMachineAlias",
            ));
        }
        self.machines.remove(given);
        Ok(json!({}))
    }

    pub(crate) fn list_machines(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let mut all: Vec<&Machine> = self
            .machines
            .iter()
            .map(|(_, m)| m)
            .filter(|m| {
                arn::parts(&m.arn)
                    .is_some_and(|p| p.region == r.region() && p.account == r.account())
            })
            .collect();
        all.sort_by(|a, b| a.name.cmp(&b.name));
        let (from, size) = (r.page_from()?, r.page_size(100));
        let page: Vec<Value> = all
            .iter()
            .skip(from)
            .take(size)
            .map(|m| {
                json!({"stateMachineArn": m.arn, "name": m.name, "type": kind(m.express), "creationDate": secs(m.created_ms)})
            })
            .collect();
        let mut out = json!({"stateMachines": page});
        if from + size < all.len() {
            out["nextToken"] = json!(token(from + size));
        }
        Ok(out)
    }

    /// `ValidateStateMachineDefinition`: every diagnostic, in AWS's order.
    pub(crate) fn validate(r: &Req<'_>) -> Value {
        let diags = match aws_asl::parse(r.s("definition")) {
            Ok(_) => Vec::new(),
            Err(d) => d,
        };
        let most = r.page_size(100);
        let list: Vec<Value> = diags
            .iter()
            .take(most)
            .map(|d| {
                let mut m = Map::new();
                m.insert(
                    "severity".into(),
                    json!(if d.warning { "WARNING" } else { "ERROR" }),
                );
                m.insert("code".into(), json!(d.code));
                m.insert("message".into(), json!(d.message));
                put(&mut m, "location", d.location.clone().map(Value::String));
                Value::Object(m)
            })
            .collect();
        let failed = diags.iter().any(|d| !d.warning);
        json!({"result": if failed { "FAIL" } else { "OK" }, "diagnostics": list, "truncated": diags.len() > most})
    }

    pub(crate) fn machine_for_execution(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let e = self.execution(r.s("executionArn"))?;
        let mut out = Map::new();
        out.insert(
            "stateMachineArn".into(),
            json!(
                e.version_arn
                    .clone()
                    .unwrap_or_else(|| e.machine_arn.clone())
            ),
        );
        out.insert(
            "name".into(),
            json!(e.machine_arn.rsplit(':').next().unwrap_or_default()),
        );
        out.insert("definition".into(), json!(e.definition));
        out.insert("roleArn".into(), json!(e.role_arn));
        out.insert("updateDate".into(), secs(e.start_ms));
        out.insert(
            "loggingConfiguration".into(),
            block(e.logging.as_deref(), logging_default()),
        );
        out.insert("tracingConfiguration".into(), json!({"enabled": false}));
        out.insert(
            "encryptionConfiguration".into(),
            json!({"type": "AWS_OWNED_KEY"}),
        );
        put(&mut out, "mapRunArn", e.map_run.clone().map(Value::String));
        Ok(Value::Object(out))
    }
}

pub(crate) const fn kind(express: bool) -> &'static str {
    if express { "EXPRESS" } else { "STANDARD" }
}

fn logging_default() -> Value {
    json!({"level": "OFF", "includeExecutionData": false})
}

/// A stored configuration block, or AWS's default.
fn block(stored: Option<&str>, default: Value) -> Value {
    stored
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(default)
}

fn describe(m: &Machine) -> Value {
    let mut out = Map::new();
    out.insert("stateMachineArn".into(), json!(m.arn));
    out.insert("name".into(), json!(m.name));
    out.insert("status".into(), json!("ACTIVE"));
    out.insert("definition".into(), json!(m.definition));
    out.insert("roleArn".into(), json!(m.role_arn));
    out.insert("type".into(), json!(kind(m.express)));
    out.insert("creationDate".into(), secs(m.created_ms));
    out.insert(
        "loggingConfiguration".into(),
        block(m.logging.as_deref(), logging_default()),
    );
    out.insert(
        "tracingConfiguration".into(),
        block(m.tracing.as_deref(), json!({"enabled": false})),
    );
    out.insert(
        "encryptionConfiguration".into(),
        block(m.encryption.as_deref(), json!({"type": "AWS_OWNED_KEY"})),
    );
    out.insert("revisionId".into(), json!(m.revision));
    put(
        &mut out,
        "description",
        m.description.clone().map(Value::String),
    );
    Value::Object(out)
}

fn describe_version(m: &Machine, v: &Version) -> Value {
    let mut out = describe(m);
    out["stateMachineArn"] = json!(format!("{}:{}", m.arn, v.number));
    out["definition"] = json!(v.definition);
    out["roleArn"] = json!(v.role_arn);
    out["creationDate"] = secs(v.created_ms);
    out["revisionId"] = json!(v.revision);
    out["loggingConfiguration"] = block(v.logging.as_deref(), logging_default());
    out["tracingConfiguration"] = block(v.tracing.as_deref(), json!({"enabled": false}));
    if let (Some(d), Some(o)) = (&v.description, out.as_object_mut()) {
        o.insert("description".into(), json!(d));
    }
    out
}

/// Publishes the machine's current definition as its next version, unless the newest version already is it (AWS answers that one).
pub(crate) fn publish(m: &mut Machine, description: Option<&str>, now: i64) -> String {
    if let Some(v) = m.versions.last()
        && v.revision == m.revision
    {
        return format!("{}:{}", m.arn, v.number);
    }
    let number = m.next_version;
    m.next_version += 1;
    m.versions.push(Version {
        number,
        definition: m.definition.clone(),
        role_arn: m.role_arn.clone(),
        description: description.map(str::to_owned),
        revision: m.revision.clone(),
        created_ms: now,
        logging: m.logging.clone(),
        tracing: m.tracing.clone(),
        encryption: m.encryption.clone(),
    });
    format!("{}:{number}", m.arn)
}

/// `Invalid Role Arn: '<arn>'` (recorded) for a role ARN that is not one.
fn check_role(role: &str) -> Result<(), ApiError> {
    let ok = role
        .strip_prefix("arn:")
        .and_then(|r| r.split_once(":iam::"))
        .is_some_and(|(_, rest)| rest.contains(":role/"));
    if ok {
        Ok(())
    } else {
        Err(invalid_arn("Role Arn", role))
    }
}

fn check_definition(text: &str, express: bool) -> Result<(), ApiError> {
    let def = aws_asl::parse(text).map_err(|d| invalid_definition(aws_asl::def::refusal(&d)))?;
    if express {
        let mut all = Vec::new();
        aws_asl::def::resources(&def, &mut all);
        if let Some(bad) = all
            .iter()
            .find(|r| r.contains(".sync") || r.ends_with(".waitForTaskToken"))
        {
            // (memory)
            return Err(invalid_definition(format!(
                "Invalid State Machine Definition: 'SCHEMA_VALIDATION_FAILED: Express state machine does not support '{}' service integration at /States'",
                if bad.contains(".sync") {
                    ".sync"
                } else {
                    ".waitForTaskToken"
                }
            )));
        }
    }
    Ok(())
}

/// A logging configuration that logs must name one log group (memory: the wording).
fn check_logging(logging: Option<&str>, _express: bool) -> Result<(), ApiError> {
    let Some(v) = logging.and_then(|l| serde_json::from_str::<Value>(l).ok()) else {
        return Ok(());
    };
    let level = v.get("level").and_then(Value::as_str).unwrap_or("OFF");
    let destinations = v
        .get("destinations")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if level != "OFF" && destinations != 1 {
        return Err(ApiError::modeled(
            "InvalidLoggingConfiguration",
            "Invalid Logging Configuration: Must specify exactly one Log Destination.",
        ));
    }
    Ok(())
}
