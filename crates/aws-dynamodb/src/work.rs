//! Work a call leaves for after its answer is made, done outside the region's actor: sending item changes to a Kinesis stream,
//! checking that a stream exists, writing an export to S3 and reading an import from it. The actor never waits on another service
//! (S3 can start a function that calls DynamoDB back); what the work finds is handed back to it as a [`Done`], and the caller's
//! answer is sent once that is in, so a describe right after the call sees the task finished.

use std::sync::Arc;

use aws_peers::Peers;
use aws_wire::service::Caller;
use serde_json::{Value as Json, json};

/// What is left to do.
#[derive(Debug)]
pub(crate) enum Work {
    /// Records for a Kinesis stream: each its partition key and data.
    Deliver {
        stream_arn: String,
        records: Vec<(String, Vec<u8>)>,
    },
    /// Whether the stream a streaming destination names exists.
    CheckStream {
        caller: Box<Caller>,
        table: String,
        stream_arn: String,
    },
    Export(Box<crate::export::Job>),
    Import(Box<crate::import::Job>),
}

/// What the work found, for the actor.
#[derive(Debug)]
pub(crate) enum Done {
    Stream {
        table: String,
        stream_arn: String,
        found: Result<(), String>,
    },
    Exported {
        arn: String,
        result: Result<crate::export::Written, Failure>,
    },
    Imported {
        arn: String,
        result: Result<crate::import::Read, Failure>,
    },
}

/// Why a task failed: the failure code and message its describe shows.
#[derive(Clone, Debug)]
pub(crate) struct Failure {
    pub(crate) code: String,
    pub(crate) message: String,
}

impl Failure {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

/// The caller a service call is made as: the account and region of the resource it acts on, as DynamoDB itself.
pub(crate) fn service_caller(arn: &str) -> Caller {
    let mut parts = arn.split(':');
    let region = parts.nth(3).unwrap_or("us-east-1").to_owned();
    let account = parts.next().unwrap_or("000000000000").to_owned();
    Caller {
        access_key_id: "dynamodb".into(),
        account_id: account,
        region,
        endpoint: String::new(),
        service: Some("dynamodb".into()),
        session_token: None,
    }
}

/// Kinesis takes at most 500 records a call.
const PUT_RECORDS_MAX: usize = 500;

/// Does the work; answers what the actor must be told.
pub(crate) async fn run(peers: Option<&Arc<Peers>>, work: Vec<Work>) -> Vec<Done> {
    let mut done = Vec::new();
    for w in work {
        let Some(peers) = peers else {
            done.extend(without_peers(w));
            continue;
        };
        match w {
            Work::Deliver {
                stream_arn,
                records,
            } => deliver(peers, &stream_arn, records).await,
            Work::CheckStream {
                caller,
                table,
                stream_arn,
            } => {
                let found = peers
                    .call(
                        "dynamodb",
                        &caller,
                        "kinesis",
                        "DescribeStreamSummary",
                        &json!({"StreamARN": stream_arn}),
                    )
                    .await
                    .map(drop)
                    .map_err(|e| e.message);
                done.push(Done::Stream {
                    table,
                    stream_arn,
                    found,
                });
            }
            Work::Export(job) => {
                let arn = job.arn.clone();
                let result = crate::export::write(peers, &job).await;
                done.push(Done::Exported { arn, result });
            }
            Work::Import(job) => {
                let arn = job.arn.clone();
                let result = crate::import::read(peers, &job).await;
                done.push(Done::Imported { arn, result });
            }
        }
    }
    done
}

/// With no other service to call (a service made alone, in a test), a task fails as it would on a missing bucket or stream.
fn without_peers(w: Work) -> Option<Done> {
    let none = "doze-aws runs no other service here";
    match w {
        Work::Deliver { .. } => None,
        Work::CheckStream {
            table, stream_arn, ..
        } => Some(Done::Stream {
            table,
            stream_arn,
            found: Err(none.to_owned()),
        }),
        Work::Export(job) => Some(Done::Exported {
            arn: job.arn,
            result: Err(Failure::new("S3NoSuchBucket", none)),
        }),
        Work::Import(job) => Some(Done::Imported {
            arn: job.arn,
            result: Err(Failure::new("S3NoSuchBucket", none)),
        }),
    }
}

async fn deliver(peers: &Arc<Peers>, stream_arn: &str, records: Vec<(String, Vec<u8>)>) {
    let caller = service_caller(stream_arn);
    for chunk in records.chunks(PUT_RECORDS_MAX) {
        let entries: Vec<Json> = chunk
            .iter()
            .map(|(key, data)| json!({"PartitionKey": key, "Data": aws_core::base64::encode(data)}))
            .collect();
        let body = json!({"StreamARN": stream_arn, "Records": entries});
        if let Err(e) = peers
            .call("dynamodb", &caller, "kinesis", "PutRecords", &body)
            .await
        {
            tracing::warn!(stream = %stream_arn, error = %e.message, "dynamodb: a Kinesis streaming destination could not be written");
        }
    }
}
