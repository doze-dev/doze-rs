//! A call written out for a person to run: the AWS CLI's command and boto3's code, from the model's names. Pure: nothing here
//! calls a service, and nothing reads a request that was made (the Live record keeps no bodies; see [`super::copy`]).

use aws_schema::tables::{K_BOOL, K_FLOAT, K_INT, K_LIST, K_MAP, K_STR, K_STRUCT, K_TS, K_UNION};
use serde_json::Value;

use super::copy::{cli_name, kebab, quote};
use aws_schema::tables::MemberRef;

/// Where and as whom a command is written to run.
#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    pub endpoint: &'a str,
    pub region: &'a str,
}

/// The members of `input` the model knows, as the model names them: `(member, value)` in the model's order.
#[must_use]
pub fn known<'a>(
    members: &'a [MemberRef<'a>],
    input: &'a Value,
) -> Vec<(&'a MemberRef<'a>, &'a Value)> {
    members
        .iter()
        .filter_map(|m| Some((m, input.get(m.wire).filter(|v| !v.is_null())?)))
        .collect()
}

/// `snake_case` as botocore names a method or a parameter of a client: `ListObjectsV2` is `list_objects_v2`.
#[must_use]
pub fn snake(name: &str) -> String {
    kebab(name).replace('-', "_")
}

/// The AWS CLI command for operation `op` of the API with SDK id `sdk_id`.
#[must_use]
pub fn cli(sdk_id: &str, op: &str, at: Target<'_>, args: &[(&MemberRef<'_>, &Value)]) -> String {
    let mut line = format!(
        "aws --endpoint-url {} --region {} {} {}",
        quote(at.endpoint),
        quote(at.region),
        cli_name(sdk_id),
        kebab(op)
    );
    for (m, v) in args {
        let flag = kebab(m.name);
        match (m.kind, v) {
            // The CLI writes a boolean as a flag and its opposite as `--no-…`.
            (K_BOOL, Value::Bool(true)) => line.push_str(&format!(" --{flag}")),
            (K_BOOL, Value::Bool(false)) => line.push_str(&format!(" --no-{flag}")),
            // A timestamp written as text (an ISO 8601 string) is one word to the CLI, as it is typed.
            (_, Value::String(s)) if m.kind == K_STR || m.kind == K_TS => {
                line.push_str(&format!(" --{flag} {}", quote(s)))
            }
            (K_INT | K_FLOAT, Value::Number(n)) => line.push_str(&format!(" --{flag} {n}")),
            // A list, a map or a structure is one JSON argument.
            _ => line.push_str(&format!(" --{flag} {}", quote(&v.to_string()))),
        }
    }
    line
}

/// A JSON value as a Python literal (`true` is `True`).
#[must_use]
pub fn python(v: &Value) -> String {
    match v {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => serde_json::to_string(s).unwrap_or_default(),
        Value::Array(a) => format!("[{}]", a.iter().map(python).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{}: {}", python(&Value::String(k.clone())), python(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// boto3's name for the client of the API with SDK id `sdk_id` (the CLI's, but S3's is `s3`).
fn client_name(sdk_id: &str) -> String {
    match cli_name(sdk_id).as_str() {
        "s3api" => "s3".to_owned(),
        other => other.to_owned(),
    }
}

/// boto3 code that makes operation `op`.
#[must_use]
pub fn boto3(sdk_id: &str, op: &str, at: Target<'_>, args: &[(&MemberRef<'_>, &Value)]) -> String {
    let client = client_name(sdk_id);
    // `lambda` is a Python keyword: the client of Lambda is held in `lambda_client`.
    let var = if client == "lambda" {
        "lambda_client"
    } else {
        client.as_str()
    };
    let call = |inner: String| format!("response = {var}.{}({inner})", snake(op));
    let pairs: Vec<String> = args
        .iter()
        .map(|(m, v)| format!("{}={}", m.name, python(v)))
        .collect();
    let single = call(pairs.join(", "));
    let call = if single.len() <= 88 {
        single
    } else {
        call(format!("\n    {}\n", pairs.join(",\n    ")))
    };
    format!(
        "import boto3\n\n{var} = boto3.client(\n    {},\n    endpoint_url={},\n    region_name={},\n)\n{call}\n",
        python(&Value::String(client.clone())),
        python(&Value::String(at.endpoint.to_owned())),
        python(&Value::String(at.region.to_owned())),
    )
}

/// Whether a member holds more than one value in a form (a list, a map, a structure).
#[must_use]
pub const fn nested(kind: u8) -> bool {
    matches!(kind, K_LIST | K_MAP | K_STRUCT | K_UNION)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{python, snake};

    #[test]
    fn a_json_value_is_a_python_literal() {
        assert_eq!(
            python(&json!({"a": [true, null, 1, "x\"y"]})),
            r#"{"a": [True, None, 1, "x\"y"]}"#
        );
    }

    #[test]
    fn a_method_is_named_as_botocore_names_it() {
        assert_eq!(snake("CreateQueue"), "create_queue");
        assert_eq!(snake("ListObjectsV2"), "list_objects_v2");
        assert_eq!(snake("GetQueueURL"), "get_queue_url");
    }
}
