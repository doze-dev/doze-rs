//! `GET /api/v1/calls/<seq>/copy`: a call of the record as an AWS CLI command and a curl command.
//!
//! The record keeps no request body (it would weigh what the bodies weigh, and hold whatever secrets they carried), so a command
//! is what the record has: the operation, the region, the endpoint, and the one input member that named the resource. The answer
//! says which members are filled in; the person adds the rest. Worked out when asked, never on the request path.

use aws_schema::meta::Protocol;
use aws_schema::tables::Tables;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;

use super::{error, json};
use crate::model::CopyCommands;
use crate::ring::Call;
use crate::state::Shared;

/// The AWS CLI's name for an API whose name is not its SDK id in lower case without spaces.
const CLI_NAMES: &[(&str, &str)] = &[
    ("SFN", "stepfunctions"),
    ("CloudWatch Logs", "logs"),
    ("EventBridge", "events"),
    ("S3", "s3api"),
];

/// The AWS CLI's name for the API with SDK id `sdk_id`.
#[must_use]
pub fn cli_name(sdk_id: &str) -> String {
    CLI_NAMES.iter().find(|(id, _)| *id == sdk_id).map_or_else(
        || sdk_id.replace(' ', "").to_lowercase(),
        |(_, n)| (*n).to_owned(),
    )
}

/// `SendMessage` as the CLI writes it: `send-message` (`ListObjectsV2` is `list-objects-v2`, `stateMachineArn` is
/// `state-machine-arn`).
#[must_use]
pub fn kebab(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|p| chars.get(p)).copied();
        let next = chars.get(i + 1).copied();
        let starts = match prev {
            None => false,
            Some(p) if c.is_ascii_uppercase() => {
                p.is_ascii_lowercase()
                    || p.is_ascii_digit()
                    || (p.is_ascii_uppercase() && next.is_some_and(|n| n.is_ascii_lowercase()))
            }
            Some(_) => false,
        };
        if starts {
            out.push('-');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// `text` as one word of a POSIX shell.
#[must_use]
pub fn quote(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./:@%+=,-".contains(c));
    if plain {
        return text.to_owned();
    }
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// `text` percent-encoded for a URL path segment or a form value.
fn encode(text: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) || (keep_slash && b == b'/') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The members and values a call is known to have carried.
fn known(call: &Call, sdk_id: &str) -> Vec<(String, String)> {
    if call.resource.is_empty() {
        return Vec::new();
    }
    if sdk_id == "S3" && call.member.is_empty() {
        let (bucket, key) = call
            .resource
            .split_once('/')
            .unwrap_or((&call.resource, ""));
        let mut out = vec![("Bucket".to_owned(), bucket.to_owned())];
        if !key.is_empty() {
            out.push(("Key".to_owned(), key.to_owned()));
        }
        return out;
    }
    if call.member.is_empty() {
        return Vec::new();
    }
    vec![(call.member.to_owned(), call.resource.to_string())]
}

/// The curl command for `call` to an API that speaks `protocols`, at `endpoint`.
fn curl(
    call: &Call,
    tables: &Tables,
    meta: &aws_schema::meta::ServiceMeta,
    endpoint: &str,
    args: &[(String, String)],
) -> Option<String> {
    let sign = format!(
        "curl -sS --aws-sigv4 {} --user \"$AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY\"",
        quote(&format!("aws:amz:{}:{}", call.region, meta.signing_name))
    );
    let op = call.action;
    let protocol = meta
        .protocols
        .iter()
        .find(|p| !matches!(p, Protocol::Rpcv2Cbor))?;
    match protocol {
        Protocol::AwsJson10 | Protocol::AwsJson11 => {
            let version = if matches!(protocol, Protocol::AwsJson10) {
                "1.0"
            } else {
                "1.1"
            };
            let body: serde_json::Map<String, serde_json::Value> = args
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::from(v.as_str())))
                .collect();
            Some(format!(
                "{sign} -H {} -H {} -d {} {endpoint}/",
                quote(&format!("Content-Type: application/x-amz-json-{version}")),
                quote(&format!("X-Amz-Target: {}.{op}", meta.shape_name)),
                quote(&serde_json::Value::Object(body).to_string()),
            ))
        }
        Protocol::AwsQuery => {
            let mut form = format!("Action={op}&Version={}", meta.version);
            for (k, v) in args {
                form.push_str(&format!("&{k}={}", encode(v, false)));
            }
            Some(format!("{sign} -d {} {endpoint}/", quote(&form)))
        }
        _ => {
            let at = tables.op(tables.find_op(op)?)?;
            let mut uri = at.uri.to_owned();
            for (k, v) in args {
                uri = uri
                    .replace(&format!("{{{k}+}}"), &encode(v, true))
                    .replace(&format!("{{{k}}}"), &encode(v, false));
            }
            let method = if at.method.is_empty() {
                "GET"
            } else {
                at.method
            };
            Some(format!(
                "{sign} -X {method} {}",
                quote(&format!("{endpoint}{uri}"))
            ))
        }
    }
}

/// The commands of `call`.
#[must_use]
pub fn commands(call: &Call, s: &crate::state::State) -> CopyCommands {
    let api = s.facts.api(call.sdk_id);
    let args = known(call, call.sdk_id);
    let endpoint = s.facts.endpoint.trim_end_matches('/');
    let cli = (!call.action.is_empty() && !call.sdk_id.is_empty()).then(|| {
        let mut line = format!(
            "aws --endpoint-url {} --region {} {} {}",
            quote(endpoint),
            quote(&call.region),
            cli_name(call.sdk_id),
            kebab(call.action)
        );
        for (k, v) in &args {
            line.push_str(&format!(" --{} {}", kebab(k), quote(v)));
        }
        line
    });
    let curl = api
        .filter(|_| !call.action.is_empty())
        .and_then(|a| curl(call, a.tables(), a.meta(), endpoint, &args));
    CopyCommands {
        seq: call.seq,
        cli,
        curl,
        filled: args.into_iter().map(|(k, _)| k).collect(),
    }
}

/// `GET /api/v1/calls/<seq>/copy`.
pub async fn copy(State(s): State<Shared>, Path(seq): Path<String>) -> Response {
    let Some(call) = seq.parse().ok().and_then(|n| s.recorder.get(n)) else {
        return error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "The record no longer holds this call (it keeps only the newest), or there is no such call.",
        );
    };
    json(StatusCode::OK, &commands(&call, &s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_written_as_the_cli_writes_them() {
        assert_eq!(kebab("SendMessage"), "send-message");
        assert_eq!(kebab("ListObjectsV2"), "list-objects-v2");
        assert_eq!(kebab("stateMachineArn"), "state-machine-arn");
        assert_eq!(kebab("QueueUrl"), "queue-url");
        assert_eq!(kebab("GetQueueURL"), "get-queue-url");
        assert_eq!(kebab("DescribeDBInstances"), "describe-db-instances");
        assert_eq!(cli_name("SFN"), "stepfunctions");
        assert_eq!(cli_name("DynamoDB Streams"), "dynamodbstreams");
        assert_eq!(cli_name("Secrets Manager"), "secretsmanager");
    }

    #[test]
    fn a_word_is_quoted_only_when_a_shell_would_split_or_read_it() {
        assert_eq!(
            quote("http://127.0.0.1:4566/000000000000/q"),
            "http://127.0.0.1:4566/000000000000/q"
        );
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote(""), "''");
    }
}
