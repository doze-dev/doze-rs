//! Dynamic references: `{{resolve:ssm:name[:version]}}`, `{{resolve:ssm-secure:name[:version]}}` and
//! `{{resolve:secretsmanager:id[:SecretString[:json-key[:version-stage[:version-id]]]]}}`, resolved from the local SSM and Secrets
//! Manager when a resource is made or updated, never stored in the template. A reference that cannot be resolved fails the
//! resource; it never becomes an empty string.

use serde_json::{Value, json};

use crate::j::J;
use aws_peers::Peers;
use aws_wire::service::Caller;

const ACTING: &str = "cloudformation";

/// The value of an SSM parameter (`name` may carry `:version`, or be the parameter's ARN).
///
/// # Errors
/// Why it could not be read.
pub(crate) async fn ssm_value(
    peers: Option<&Peers>,
    caller: &Caller,
    name: &str,
    decrypt: bool,
) -> Result<String, String> {
    let peers = peers.ok_or("SSM is not running")?;
    let name = name
        .strip_prefix("arn:")
        .and_then(|a| a.split_once(":parameter").map(|(_, n)| n.to_owned()))
        .unwrap_or_else(|| name.to_owned());
    let got = peers
        .call(
            ACTING,
            caller,
            "ssm",
            "GetParameter",
            &json!({"Name": name, "WithDecryption": decrypt}),
        )
        .await
        .map_err(|e| e.message)?;
    got.at("Parameter")
        .at("Value")
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("parameter {name} has no value"))
}

/// Every dynamic reference in `v` replaced by its value.
///
/// # Errors
/// The first reference that could not be resolved, in AWS's words where known.
pub(crate) async fn resolve(
    v: &mut Value,
    peers: Option<&Peers>,
    caller: &Caller,
) -> Result<(), String> {
    match v {
        Value::String(s) if s.contains("{{resolve:") => {
            *s = resolve_text(s, peers, caller).await?;
        }
        Value::Array(a) => {
            for x in a {
                Box::pin(resolve(x, peers, caller)).await?;
            }
        }
        Value::Object(o) => {
            for x in o.values_mut() {
                Box::pin(resolve(x, peers, caller)).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn resolve_text(s: &str, peers: Option<&Peers>, caller: &Caller) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("{{resolve:") {
        out.push_str(rest.get(..start).unwrap_or_default());
        let after = rest.get(start + 10..).unwrap_or_default();
        let Some(end) = after.find("}}") else {
            out.push_str(rest.get(start..).unwrap_or_default());
            return Ok(out);
        };
        let reference = after.get(..end).unwrap_or_default();
        out.push_str(&one(reference, peers, caller).await?);
        rest = after.get(end + 2..).unwrap_or_default();
    }
    out.push_str(rest);
    Ok(out)
}

/// One reference, without its braces and `resolve:`.
async fn one(reference: &str, peers: Option<&Peers>, caller: &Caller) -> Result<String, String> {
    let (service, rest) = reference.split_once(':').unwrap_or((reference, ""));
    match service {
        "ssm" | "ssm-secure" => {
            let (name, version) = split_version(rest);
            let qualified = match version {
                Some(v) => format!("{name}:{v}"),
                None => name.to_owned(),
            };
            ssm_value(peers, caller, &qualified, service == "ssm-secure")
                .await
                .map_err(|_| {
                    // (memory)
                    format!("Parameters: [ssm:{name}] cannot be found.")
                })
        }
        "secretsmanager" => secret(rest, peers, caller).await,
        // (memory)
        other => Err(format!("Unrecognized dynamic reference service: {other}")),
    }
}

/// A parameter name and an optional trailing `:version` (a name is a path or plain word; an ARN keeps its own colons).
fn split_version(rest: &str) -> (&str, Option<&str>) {
    match rest.rsplit_once(':') {
        Some((name, v))
            if !v.is_empty()
                && v.bytes().all(|c| c.is_ascii_digit())
                && !rest.starts_with("arn:") =>
        {
            (name, Some(v))
        }
        Some((name, v))
            if rest.starts_with("arn:")
                && rest.matches(':').count() > 5
                && v.bytes().all(|c| c.is_ascii_digit()) =>
        {
            (name, Some(v))
        }
        _ => (rest, None),
    }
}

async fn secret(rest: &str, peers: Option<&Peers>, caller: &Caller) -> Result<String, String> {
    let peers = peers.ok_or("Secrets Manager is not running")?;
    // An ARN carries seven colon-separated parts of its own.
    let parts: Vec<&str> = rest.split(':').collect();
    let (id, tail): (String, &[&str]) = if rest.starts_with("arn:") && parts.len() >= 7 {
        (
            parts.get(..7).unwrap_or_default().join(":"),
            parts.get(7..).unwrap_or_default(),
        )
    } else {
        (
            parts.first().copied().unwrap_or_default().to_owned(),
            parts.get(1..).unwrap_or_default(),
        )
    };
    let field = |i: usize| tail.get(i).copied().filter(|s| !s.is_empty());
    if let Some(f) = field(0)
        && f != "SecretString"
    {
        // (memory)
        return Err(format!(
            "Could not parse SecretString JSON: unsupported field {f}"
        ));
    }
    let mut body = json!({"SecretId": id});
    if let Some(stage) = field(2) {
        body.put("VersionStage", stage.into());
    }
    if let Some(version) = field(3) {
        body.put("VersionId", version.into());
    }
    let got = peers
        .call(ACTING, caller, "secretsmanager", "GetSecretValue", &body)
        .await
        .map_err(|e| format!("Secrets Manager can't find the specified secret. (Service: AWSSecretsManager; Status Code: {}; Error Code: {})", e.status, e.code))?;
    let text = got
        .at("SecretString")
        .as_str()
        .unwrap_or_default()
        .to_owned();
    match field(1) {
        None => Ok(text),
        Some(key) => {
            let doc: Value = serde_json::from_str(&text)
                .map_err(|_| "Could not parse SecretString JSON".to_owned())?; // (memory)
            match doc.get(key) {
                Some(Value::String(s)) => Ok(s.clone()),
                Some(other) => Ok(other.to_string()),
                // (memory)
                None => Err(format!("Could not find SecretString JSON key {key}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::split_version;

    #[test]
    fn a_version_is_the_last_number() {
        assert_eq!(split_version("/a/b:3"), ("/a/b", Some("3")));
        assert_eq!(split_version("/a/b"), ("/a/b", None));
        assert_eq!(split_version("name"), ("name", None));
    }
}
