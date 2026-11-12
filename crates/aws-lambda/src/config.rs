//! A function's configuration: which request members set it, the checks beyond the model, and how it is shown with its defaults.

use aws_core::time::format_iso8601;
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::errors::invalid;
use crate::model::{Code, Snap};
use crate::req::Req;

/// The runtimes AWS accepts for a new function, in the order its validation message lists them (recorded 2026-11-09). The model also
/// lists the runtimes AWS has since retired; creating or updating a function with one of those is refused.
pub(crate) const RUNTIMES: &str = "nodejs20.x, python3.14, provided.al2023, python3.15, python3.12, python3.13, nodejs24.x, nodejs22.x, nodejs26.x, java25, python3.10, python3.11, java21, ruby3.3, ruby3.4, ruby4.0, ruby3.2, python3.8, python3.9, java17.al2023, java17, nodejs16.x, dotnet10, dotnet8, java8.al2023, java11, dotnet6, nodejs18.x, provided.al2, java8.al2, java11.al2023";

/// The members a caller sets on a function, by `CreateFunction` and `UpdateFunctionConfiguration`. `Command` is a doze extension:
/// the launch line, replacing the one the runtime would give.
const SETTABLE: &[&str] = &[
    "Runtime",
    "Role",
    "Handler",
    "Description",
    "Timeout",
    "MemorySize",
    "Environment",
    "Layers",
    "TracingConfig",
    "DeadLetterConfig",
    "VpcConfig",
    "FileSystemConfigs",
    "KMSKeyArn",
    "Architectures",
    "EphemeralStorage",
    "LoggingConfig",
    "SnapStart",
    "PackageType",
    "ImageConfig",
    "CodeSigningConfigArn",
    "Command",
    "TenantIsolationConfig",
];

/// The environment variables Lambda sets itself and does not let a function set.
const RESERVED: &[&str] = &[
    "_HANDLER",
    "AWS_REGION",
    "AWS_EXECUTION_ENV",
    "AWS_LAMBDA_FUNCTION_NAME",
    "AWS_LAMBDA_FUNCTION_MEMORY_SIZE",
    "AWS_LAMBDA_FUNCTION_VERSION",
    "AWS_LAMBDA_INITIALIZATION_TYPE",
    "AWS_LAMBDA_LOG_GROUP_NAME",
    "AWS_LAMBDA_LOG_STREAM_NAME",
    "AWS_ACCESS_KEY",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_LAMBDA_RUNTIME_API",
    "LAMBDA_TASK_ROOT",
    "LAMBDA_RUNTIME_DIR",
];

/// The settable members of a request, as a configuration object.
pub(crate) fn from_request(r: &Req) -> Map<String, Value> {
    let mut out = Map::new();
    for key in SETTABLE {
        if let Some(v) = r.value(key) {
            out.insert((*key).to_owned(), v.clone());
        }
    }
    out
}

/// Applies the settable members of an update to `config`: each member given replaces the one stored.
pub(crate) fn merge(config: &mut Map<String, Value>, r: &Req) {
    for (k, v) in from_request(r) {
        config.insert(k, v);
    }
}

/// Checks a configuration beyond what the model's constraints say.
///
/// # Errors
/// A runtime AWS has retired, or a reserved environment variable.
pub(crate) fn check(config: &Map<String, Value>) -> Result<(), ApiError> {
    if let Some(rt) = config.get("Runtime").and_then(Value::as_str)
        && !rt.starts_with("arn:")
        && !RUNTIMES.split(", ").any(|r| r == rt)
    {
        return Err(invalid(format!(
            "The runtime parameter of {rt} is no longer supported for creating or updating AWS Lambda functions. We recommend you use a supported runtime while creating or updating functions."
        )));
    }
    let vars = config
        .get("Environment")
        .and_then(|e| e.get("Variables"))
        .and_then(Value::as_object);
    let reserved: Vec<&str> = vars
        .map(|v| {
            v.keys()
                .map(String::as_str)
                .filter(|k| RESERVED.contains(k))
                .collect()
        })
        .unwrap_or_default();
    if !reserved.is_empty() {
        return Err(invalid(format!(
            "Lambda was unable to configure your environment variables because the environment variables you have provided contains reserved keys that are currently not supported for modification. Reserved keys used in this request: {}",
            reserved.join(",")
        )));
    }
    Ok(())
}

/// `2026-11-13T10:11:12.345+0000`, the way Lambda writes a time.
pub(crate) fn stamp(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let iso = format_iso8601(secs);
    let date = iso.trim_end_matches('Z');
    let date = date.split('.').next().unwrap_or(date);
    format!("{date}.{:03}+0000", ms.rem_euclid(1000))
}

/// What a configuration shows on the wire.
pub(crate) struct Shown<'a> {
    pub(crate) name: &'a str,
    pub(crate) arn: String,
    pub(crate) version: &'a str,
    pub(crate) snap: &'a Snap,
    /// The size of each layer version, by ARN (0 for one that is not here).
    pub(crate) layer_sizes: &'a dyn Fn(&str) -> u64,
}

/// The `FunctionConfiguration` for a snapshot, defaults filled in.
pub(crate) fn render(s: &Shown<'_>) -> Value {
    let cfg: Map<String, Value> = serde_json::from_str(&s.snap.config).unwrap_or_default();
    let get = |k: &str| cfg.get(k).cloned();
    let mut out = Map::new();
    out.insert("FunctionName".into(), json!(s.name));
    out.insert("FunctionArn".into(), json!(s.arn));
    out.insert("Version".into(), json!(s.version));
    out.insert(
        "Description".into(),
        get("Description").unwrap_or(json!("")),
    );
    out.insert("Role".into(), get("Role").unwrap_or(Value::Null));
    for key in [
        "Runtime",
        "Handler",
        "KMSKeyArn",
        "DeadLetterConfig",
        "FileSystemConfigs",
        "ImageConfig",
    ] {
        if let Some(v) = get(key) {
            out.insert(key.into(), v);
        }
    }
    out.insert("Timeout".into(), get("Timeout").unwrap_or(json!(3)));
    out.insert("MemorySize".into(), get("MemorySize").unwrap_or(json!(128)));
    out.insert("CodeSize".into(), json!(s.snap.code.size));
    out.insert("CodeSha256".into(), json!(s.snap.code.sha256));
    out.insert("LastModified".into(), json!(stamp(s.snap.modified_ms)));
    out.insert("RevisionId".into(), json!(s.snap.revision));
    out.insert(
        "PackageType".into(),
        get("PackageType").unwrap_or(json!(package_type(&s.snap.code))),
    );
    out.insert(
        "Architectures".into(),
        get("Architectures").unwrap_or(json!(["x86_64"])),
    );
    out.insert(
        "EphemeralStorage".into(),
        get("EphemeralStorage").unwrap_or(json!({"Size": 512})),
    );
    out.insert("TracingConfig".into(), json!({"Mode": tracing_mode(&cfg)}));
    out.insert(
        "SnapStart".into(),
        json!({"ApplyOn": snap_start(&cfg), "OptimizationStatus": "Off"}),
    );
    out.insert("LoggingConfig".into(), logging(&cfg, s.name));
    out.insert("State".into(), json!("Active"));
    out.insert("LastUpdateStatus".into(), json!("Successful"));
    if let Some(vars) = cfg.get("Environment").and_then(|e| e.get("Variables")) {
        out.insert("Environment".into(), json!({"Variables": vars}));
    }
    if let Some(layers) = cfg.get("Layers").and_then(Value::as_array) {
        let shown: Vec<Value> = layers
            .iter()
            .filter_map(Value::as_str)
            .map(|arn| json!({"Arn": arn, "CodeSize": (s.layer_sizes)(arn)}))
            .collect();
        out.insert("Layers".into(), Value::Array(shown));
    }
    if let Some(vpc) = cfg
        .get("VpcConfig")
        .filter(|v| v.as_object().is_some_and(|o| !o.is_empty()))
    {
        out.insert("VpcConfig".into(), vpc_shown(vpc));
    }
    Value::Object(out)
}

fn package_type(code: &Code) -> &'static str {
    if code.image.is_some() { "Image" } else { "Zip" }
}

fn tracing_mode(cfg: &Map<String, Value>) -> String {
    cfg.get("TracingConfig")
        .and_then(|t| t.get("Mode"))
        .and_then(Value::as_str)
        .unwrap_or("PassThrough")
        .to_owned()
}

fn snap_start(cfg: &Map<String, Value>) -> String {
    cfg.get("SnapStart")
        .and_then(|t| t.get("ApplyOn"))
        .and_then(Value::as_str)
        .unwrap_or("None")
        .to_owned()
}

fn logging(cfg: &Map<String, Value>, name: &str) -> Value {
    let mut out = Map::new();
    out.insert("LogFormat".into(), json!("Text"));
    out.insert("LogGroup".into(), json!(format!("/aws/lambda/{name}")));
    if let Some(set) = cfg.get("LoggingConfig").and_then(Value::as_object) {
        for (k, v) in set {
            out.insert(k.clone(), v.clone());
        }
    }
    Value::Object(out)
}

fn vpc_shown(vpc: &Value) -> Value {
    json!({
        "SubnetIds": vpc.get("SubnetIds").cloned().unwrap_or(json!([])),
        "SecurityGroupIds": vpc.get("SecurityGroupIds").cloned().unwrap_or(json!([])),
        "VpcId": "",
        "Ipv6AllowedForDualStack": vpc.get("Ipv6AllowedForDualStack").cloned().unwrap_or(json!(false)),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_time_is_written_the_way_lambda_writes_it() {
        assert_eq!(stamp(1_700_000_000_123), "2023-11-14T22:13:20.123+0000");
    }

    #[test]
    fn reserved_variables_are_refused_by_name() {
        let mut cfg = Map::new();
        cfg.insert(
            "Environment".into(),
            json!({"Variables": {"AWS_REGION": "x", "OK": "y"}}),
        );
        let err = check(&cfg).unwrap_err();
        assert!(
            err.message
                .ends_with("Reserved keys used in this request: AWS_REGION"),
            "{}",
            err.message
        );
    }
}
