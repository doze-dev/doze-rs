//! Reading a resource back from the service that keeps it, through that service's own read operations, as the properties
//! CloudFormation's template gives it. A reader puts every property it knows (null for one that is not set); a property it does not
//! know is not compared. The types with a reader are the ones drift detection checks; every other type is `NOT_CHECKED`.

use std::sync::Arc;

use serde_json::{Map, Value, json};

use aws_peers::Peers;
use aws_wire::ApiError;
use aws_wire::service::Caller;

use crate::j::J;

/// What a resource is read with: the bus, as `cloudformation` acting for whoever asked for the detection.
#[derive(Debug, Clone)]
pub(crate) struct Probe {
    pub(crate) peers: Arc<Peers>,
    pub(crate) caller: Caller,
}

/// A resource as it is now.
#[derive(Debug)]
pub(crate) enum Live {
    Found(Map<String, Value>),
    /// The service says it is not there.
    Gone,
}

pub(crate) type Read = Result<Live, String>;

impl Probe {
    pub(crate) async fn call(&self, svc: &str, op: &str, body: Value) -> Result<Value, ApiError> {
        self.peers
            .call("cloudformation", &self.caller, svc, op, &body)
            .await
    }

    pub(crate) async fn rest(&self, svc: &str, op: &str, body: Value) -> Result<Value, ApiError> {
        self.peers
            .call_rest("cloudformation", &self.caller, svc, op, &body)
            .await
            .map(|r| r.json)
    }
}

/// The types drift detection checks.
pub(crate) fn readable(ty: &str) -> bool {
    matches!(
        ty,
        "AWS::SQS::Queue"
            | "AWS::SNS::Topic"
            | "AWS::SSM::Parameter"
            | "AWS::Logs::LogGroup"
            | "AWS::S3::Bucket"
            | "AWS::DynamoDB::Table"
            | "AWS::IAM::Role"
            | "AWS::Lambda::Function"
            | "AWS::SecretsManager::Secret"
            | "AWS::KMS::Key"
    )
}

/// Reads the resource `physical` of `ty`.
pub(crate) async fn read(p: &Probe, ty: &str, physical: &str) -> Read {
    match ty {
        "AWS::SQS::Queue" => queue(p, physical).await,
        "AWS::SNS::Topic" => topic(p, physical).await,
        "AWS::SSM::Parameter" => parameter(p, physical).await,
        "AWS::Logs::LogGroup" => log_group(p, physical).await,
        "AWS::S3::Bucket" => bucket(p, physical).await,
        _ => super::read_more::read(p, ty, physical).await,
    }
}

/// The answer, `Gone` when the service says the thing is not there, or the error as the reason detection failed.
pub(crate) fn found(r: Result<Value, ApiError>) -> Result<Option<Value>, String> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if crate::res::gone(&e) || e.code.contains("NonExistent") => Ok(None),
        Err(e) => Err(format!("{}: {}", e.code, e.message)),
    }
}

/// `{k: v}` tags as `[{Key, Value}]`, sorted by key.
pub(crate) fn tag_list(m: &Value) -> Value {
    let mut tags: Vec<(String, String)> = m
        .as_object()
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                .collect()
        })
        .unwrap_or_default();
    tags.sort();
    Value::Array(
        tags.into_iter()
            .map(|(k, v)| json!({"Key": k, "Value": v}))
            .collect(),
    )
}

/// The members `keys` of `from`, null where not set.
pub(crate) fn pick(from: &Value, keys: &[(&str, &str)]) -> Map<String, Value> {
    keys.iter()
        .map(|(cfn, svc)| ((*cfn).to_owned(), from.at(svc).clone()))
        .collect()
}

const QUEUE: &[(&str, &str)] = &[
    ("ContentBasedDeduplication", "ContentBasedDeduplication"),
    ("DeduplicationScope", "DeduplicationScope"),
    ("DelaySeconds", "DelaySeconds"),
    ("FifoThroughputLimit", "FifoThroughputLimit"),
    (
        "KmsDataKeyReusePeriodSeconds",
        "KmsDataKeyReusePeriodSeconds",
    ),
    ("KmsMasterKeyId", "KmsMasterKeyId"),
    ("MaximumMessageSize", "MaximumMessageSize"),
    ("MessageRetentionPeriod", "MessageRetentionPeriod"),
    (
        "ReceiveMessageWaitTimeSeconds",
        "ReceiveMessageWaitTimeSeconds",
    ),
    ("RedriveAllowPolicy", "RedriveAllowPolicy"),
    ("RedrivePolicy", "RedrivePolicy"),
    ("SqsManagedSseEnabled", "SqsManagedSseEnabled"),
    ("VisibilityTimeout", "VisibilityTimeout"),
];

async fn queue(p: &Probe, url: &str) -> Read {
    let got = p
        .call(
            "sqs",
            "GetQueueAttributes",
            json!({"QueueUrl": url, "AttributeNames": ["All"]}),
        )
        .await;
    let Some(v) = found(got)? else {
        return Ok(Live::Gone);
    };
    let attrs = v.at("Attributes");
    let mut out = pick(attrs, QUEUE);
    out.insert(
        "FifoQueue".into(),
        attrs
            .get("FifoQueue")
            .cloned()
            .unwrap_or_else(|| "false".into()),
    );
    out.insert(
        "QueueName".into(),
        url.rsplit('/').next().unwrap_or_default().into(),
    );
    let tags = p
        .call("sqs", "ListQueueTags", json!({"QueueUrl": url}))
        .await
        .map_err(|e| e.message)?;
    out.insert("Tags".into(), tag_list(tags.at("Tags")));
    Ok(Live::Found(out))
}

async fn topic(p: &Probe, arn: &str) -> Read {
    let got = p
        .call("sns", "GetTopicAttributes", json!({"TopicArn": arn}))
        .await;
    let Some(v) = found(got)? else {
        return Ok(Live::Gone);
    };
    let attrs = v.at("Attributes");
    let mut out = pick(
        attrs,
        &[
            ("DisplayName", "DisplayName"),
            ("KmsMasterKeyId", "KmsMasterKeyId"),
            ("ContentBasedDeduplication", "ContentBasedDeduplication"),
            ("SignatureVersion", "SignatureVersion"),
        ],
    );
    out.insert(
        "FifoTopic".into(),
        attrs
            .get("FifoTopic")
            .cloned()
            .unwrap_or_else(|| "false".into()),
    );
    out.insert(
        "TopicName".into(),
        arn.rsplit(':').next().unwrap_or_default().into(),
    );
    let tags = p
        .call("sns", "ListTagsForResource", json!({"ResourceArn": arn}))
        .await
        .map_err(|e| e.message)?;
    out.insert("Tags".into(), tags.at("Tags").clone());
    Ok(Live::Found(out))
}

async fn parameter(p: &Probe, name: &str) -> Read {
    let got = p.call("ssm", "GetParameter", json!({"Name": name})).await;
    let Some(v) = found(got)? else {
        return Ok(Live::Gone);
    };
    let mut out = pick(
        v.at("Parameter"),
        &[
            ("Name", "Name"),
            ("Type", "Type"),
            ("Value", "Value"),
            ("DataType", "DataType"),
        ],
    );
    let filter =
        json!({"ParameterFilters": [{"Key": "Name", "Option": "Equals", "Values": [name]}]});
    let described = p
        .call("ssm", "DescribeParameters", filter)
        .await
        .map_err(|e| e.message)?;
    let meta = described
        .at("Parameters")
        .as_array()
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or(Value::Null);
    out.extend(pick(
        &meta,
        &[
            ("Description", "Description"),
            ("AllowedPattern", "AllowedPattern"),
            ("Tier", "Tier"),
        ],
    ));
    let tags = p
        .call(
            "ssm",
            "ListTagsForResource",
            json!({"ResourceType": "Parameter", "ResourceId": name}),
        )
        .await
        .map_err(|e| e.message)?;
    // CloudFormation gives a parameter's tags as an object.
    let map: Map<String, Value> = tags
        .at("TagList")
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| {
            (
                crate::res::text(t, "Key").unwrap_or_default(),
                t.at("Value").clone(),
            )
        })
        .collect();
    out.insert("Tags".into(), Value::Object(map));
    Ok(Live::Found(out))
}

async fn log_group(p: &Probe, name: &str) -> Read {
    let got = p
        .call(
            "logs",
            "DescribeLogGroups",
            json!({"logGroupNamePrefix": name}),
        )
        .await
        .map_err(|e| e.message)?;
    let Some(g) = got.at("logGroups").as_array().and_then(|a| {
        a.iter()
            .find(|g| g.at("logGroupName").as_str() == Some(name))
    }) else {
        return Ok(Live::Gone);
    };
    let mut out = pick(
        g,
        &[
            ("LogGroupName", "logGroupName"),
            ("RetentionInDays", "retentionInDays"),
            ("KmsKeyId", "kmsKeyId"),
            ("LogGroupClass", "logGroupClass"),
        ],
    );
    let arn = g
        .at("arn")
        .as_str()
        .unwrap_or_default()
        .trim_end_matches(":*")
        .to_owned();
    if let Ok(t) = p
        .call("logs", "ListTagsForResource", json!({"resourceArn": arn}))
        .await
    {
        out.insert("Tags".into(), tag_list(t.at("tags")));
    }
    Ok(Live::Found(out))
}

async fn bucket(p: &Probe, name: &str) -> Read {
    let Some(_) = found(p.rest("s3", "HeadBucket", json!({"Bucket": name})).await)? else {
        return Ok(Live::Gone);
    };
    let mut out = Map::new();
    out.insert("BucketName".into(), name.into());
    let v = p
        .rest("s3", "GetBucketVersioning", json!({"Bucket": name}))
        .await
        .map_err(|e| e.message)?;
    out.insert(
        "VersioningConfiguration".into(),
        match v.get("Status") {
            Some(s) => json!({"Status": s}),
            None => Value::Null,
        },
    );
    let tags = match p
        .rest("s3", "GetBucketTagging", json!({"Bucket": name}))
        .await
    {
        Ok(t) => t.at("TagSet").clone(),
        Err(_) => json!([]),
    };
    out.insert("Tags".into(), tags);
    Ok(Live::Found(out))
}
