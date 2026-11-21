//! CloudFormation: what deleting a stack leaves behind, read from its template: the resources whose `DeletionPolicy` is `Retain`
//! (or `RetainExceptOnCreate`), which CloudFormation lets go of without deleting. A JSON template is read as JSON; a YAML one
//! line by line, for the logical ids and their `DeletionPolicy` (nothing is evaluated: a policy set by a parameter is not seen).

use serde_json::Value;

/// The lines "`Id` (Type) stays" for the answer of `GetTemplate`.
pub(super) fn retained(answer: &Value) -> Vec<String> {
    let Some(body) = answer.get("TemplateBody") else {
        return Vec::new();
    };
    // A JSON template comes as an object; a YAML one (or a JSON one as text) as a string.
    let found = match body {
        Value::Object(_) => from_json(body),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .map(|v| from_json(&v))
            .unwrap_or_else(|_| from_yaml(text)),
        _ => Vec::new(),
    };
    found
        .into_iter()
        .map(|(id, kind)| {
            if kind.is_empty() {
                format!("{id} is retained: its DeletionPolicy is Retain.")
            } else {
                format!("{id} ({kind}) is retained: its DeletionPolicy is Retain.")
            }
        })
        .collect()
}

fn keeps(policy: &str) -> bool {
    matches!(
        policy.trim_matches(['"', '\'']),
        "Retain" | "RetainExceptOnCreate"
    )
}

fn from_json(template: &Value) -> Vec<(String, String)> {
    template
        .get("Resources")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, r)| {
            r.get("DeletionPolicy")
                .and_then(Value::as_str)
                .is_some_and(keeps)
        })
        .map(|(id, r)| {
            (
                id.clone(),
                r.get("Type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect()
}

/// One resource read from a YAML template: its logical id, its `Type`, and whether its `DeletionPolicy` retains it.
struct Seen {
    id: String,
    kind: String,
    keep: bool,
}

/// The resources of a YAML template: the keys one level under `Resources:` are the logical ids, and the `Type` and
/// `DeletionPolicy` among the lines below each are theirs.
fn from_yaml(text: &str) -> Vec<(String, String)> {
    let mut in_resources = false;
    // The indent of the logical ids: that of the first line under `Resources:`.
    let mut ids_at: Option<usize> = None;
    let (mut done, mut current): (Vec<Seen>, Option<Seen>) = (Vec::new(), None);
    for line in text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
    {
        let body = line.trim_start();
        let indent = line.len() - body.len();
        if indent == 0 {
            done.extend(current.take());
            in_resources = body.starts_with("Resources:");
            ids_at = None;
            continue;
        }
        let (true, Some((key, value))) = (in_resources, body.split_once(':')) else {
            continue;
        };
        let value = value.split('#').next().unwrap_or_default().trim();
        if *ids_at.get_or_insert(indent) == indent {
            done.extend(current.take());
            current = Some(Seen {
                id: key.trim_matches(['"', '\'']).to_owned(),
                kind: String::new(),
                keep: false,
            });
        } else if let Some(c) = current.as_mut() {
            match key {
                "Type" => c.kind = value.to_owned(),
                "DeletionPolicy" => c.keep = keeps(value),
                _ => {}
            }
        }
    }
    done.extend(current);
    done.into_iter()
        .filter(|s| s.keep)
        .map(|s| (s.id, s.kind))
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::retained;

    #[test]
    fn a_json_template_names_the_resources_it_retains() {
        let t = json!({"Resources": {
            "Data": {"Type": "AWS::S3::Bucket", "DeletionPolicy": "Retain"},
            "Queue": {"Type": "AWS::SQS::Queue"},
            "Log": {"Type": "AWS::Logs::LogGroup", "DeletionPolicy": "Delete"}}});
        assert_eq!(
            retained(&json!({"TemplateBody": t})),
            ["Data (AWS::S3::Bucket) is retained: its DeletionPolicy is Retain."]
        );
        assert_eq!(retained(&json!({"TemplateBody": t.to_string()})).len(), 1);
        assert!(retained(&json!({})).is_empty());
    }

    #[test]
    fn a_yaml_template_is_read_for_its_retain_policies() {
        let y = "AWSTemplateFormatVersion: '2010-09-09'\nResources:\n  Data:\n    Type: AWS::S3::Bucket\n    DeletionPolicy: Retain\n  Queue:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: q\n  Table:\n    DeletionPolicy: \"Retain\" # keep it\n    Type: AWS::DynamoDB::Table\nOutputs:\n  X:\n    Value: 1\n";
        assert_eq!(
            retained(&json!({"TemplateBody": y})),
            [
                "Data (AWS::S3::Bucket) is retained: its DeletionPolicy is Retain.",
                "Table (AWS::DynamoDB::Table) is retained: its DeletionPolicy is Retain.",
            ]
        );
    }
}
