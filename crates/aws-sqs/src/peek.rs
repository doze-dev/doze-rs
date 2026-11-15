//! The local look at a queue (doze only, not AWS): `GET /_doze/sqs/<region>/<queue>/peek[?max=N]` reads the messages a receive
//! could be given now without receiving them, for the console and for tests. It is not on SQS's endpoint: AWS's endpoint answers
//! AWS's operations alone.
//!
//! The answer is `ReceiveMessage`'s JSON (`{"Messages": [...]}`, absent when there are none): every visible message (not in
//! flight, not delayed, not past its retention), every group's and not only the head of each, oldest first, with every system
//! attribute but `SenderId` (a look has no sender to name) and every message attribute. Nothing changes: no receive count, no
//! visibility timeout, no new handle; a message that was received before carries its latest handle. `max` is 1 to 100, 10 when
//! absent. Unsigned, and guarded as every `/_doze/` address is (`aws_wire::local::guard`, applied by the gateway): under
//! `--auth verify` it needs the secret key in `X-Doze-Key`.

use aws_model_sqs::Operation;
use aws_model_sqs::tables::TABLES;
use aws_wire::service::{BoxFuture, Reply, Web, WebReply, WebRequest};
use serde_json::{Value, json};

use crate::Sqs;
use crate::core::{Ask, Core};
use crate::messages::{Taken, taken_from, write_received};

/// Where the hook lives. `_` cannot begin a bucket name, so no S3 path-style address is taken.
const PATH: &str = "/_doze/sqs/";
/// The most messages one look returns (doze's bound: the console asks for 50).
const MAX_PEEK: usize = 100;
const DEFAULT_PEEK: usize = 10;
/// The system attributes a look shows: all of them but `SenderId`.
const SYSTEM: &[&str] = &[
    "SentTimestamp",
    "ApproximateReceiveCount",
    "ApproximateFirstReceiveTimestamp",
    "AWSTraceHeader",
    "MessageGroupId",
    "MessageDeduplicationId",
    "SequenceNumber",
    "DeadLetterQueueSourceArn",
];

impl Core {
    /// The messages of queue `name` a receive could be given now, at most `max`, read without receiving them.
    pub(crate) fn peek(&self, name: &str, max: usize) -> Reply {
        let now = self.now_ms();
        let queue = self
            .queues
            .get(name)
            .ok_or_else(crate::core::no_such_queue)?;
        let taken: Vec<Taken> = queue
            .messages
            .iter()
            .filter(|m| m.visible_at_ms <= now && !queue.expired(m, now))
            .take(max)
            .map(|m| {
                let handle = m.handles.last().cloned().unwrap_or_default();
                taken_from(m, handle, (SYSTEM, &["All"]), "")
            })
            .collect();
        write_received(&taken)
    }
}

fn answer(status: u16, body: &Value) -> WebReply {
    aws_wire::local::reply(status, body)
}

fn refused(status: u16, code: &str, message: &str) -> WebReply {
    answer(status, &json!({"Code": code, "Message": message}))
}

/// `max`, when the query string gives it: 1 to 100.
fn max_of(query: &str) -> Result<usize, WebReply> {
    match aws_wire::query::parse_form(query).get("max") {
        None => Ok(DEFAULT_PEEK),
        Some(n) => n
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=MAX_PEEK).contains(n))
            .ok_or_else(|| {
                refused(
                    400,
                    "InvalidParameterValue",
                    "Value for parameter max is invalid. Reason: Must be between 1 and 100, if provided.",
                )
            }),
    }
}

/// `ReceiveMessage`'s output written as JSON.
fn to_json(reply: Reply) -> WebReply {
    let out = match reply {
        Ok(out) => out,
        Err(e) => return refused(e.status, e.shape.as_deref().unwrap_or(&e.code), &e.message),
    };
    let shape = TABLES
        .op(Operation::ReceiveMessage.index())
        .and_then(|o| o.output);
    let mut bytes = Vec::new();
    match shape {
        Some(shape) if out.root != aws_schema::arena::NIL => {
            if aws_schema::enc::encode(&TABLES, shape, &out.arena, "", out.root, &mut bytes)
                .is_err()
            {
                return refused(500, "InternalFailure", "The messages could not be written.");
            }
        }
        _ => bytes.extend_from_slice(b"{}"),
    }
    answer(200, &serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

impl Sqs {
    async fn serve_peek(&self, request: &WebRequest) -> WebReply {
        let rest = request.path.strip_prefix(PATH).unwrap_or_default();
        let parts: Vec<&str> = rest.split('/').collect();
        let [region, queue, "peek"] = parts.as_slice() else {
            return refused(
                404,
                "UnknownOperation",
                &format!(
                    "doze has no SQS hook at {}: use GET {PATH}<region>/<queue>/peek?max=N.",
                    request.path
                ),
            );
        };
        if request.method != "GET" {
            return refused(405, "MethodNotAllowed", "Look at a queue with GET.");
        }
        let max = match max_of(&request.query) {
            Ok(max) => max,
            Err(refusal) => return refusal,
        };
        // Only a region that holds something is woken: a look never makes a region's files.
        if !self.regions().iter().any(|r| r == region) {
            return to_json(Err(crate::core::no_such_queue()));
        }
        let Ok(actor) = self.actors.actor(region) else {
            return refused(500, "InternalFailure", "SQS could not start.");
        };
        let ask = Ask::Peek {
            name: aws_core::uri::decode_str(queue)
                .map_or_else(|| (*queue).to_owned(), std::borrow::Cow::into_owned),
            max,
        };
        match actor.call(ask).await {
            Ok(answer) => to_json(answer.reply),
            Err(e) => refused(500, "InternalFailure", &e.to_string()),
        }
    }
}

impl Web for Sqs {
    fn owns(&self, _host: &str, path: &str) -> bool {
        path.starts_with(PATH)
    }

    /// A look reads message bodies: guarded as `/_doze/data` is.
    fn local(&self) -> bool {
        true
    }

    fn serve(&self, request: WebRequest) -> BoxFuture<'_, WebReply> {
        Box::pin(async move { self.serve_peek(&request).await })
    }
}
