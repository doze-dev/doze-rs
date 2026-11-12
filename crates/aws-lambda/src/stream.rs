//! `InvokeWithResponseStream`: the answer is an event stream, and the function's response is sent on as the function writes it (a
//! runtime that posts its response in `streaming` mode), each piece as a `PayloadChunk`, ended by an `InvokeComplete` that carries
//! the error the function ended in. A function that does not stream answers with one chunk.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use aws_wire::ApiError;
use aws_wire::rest::{ByteStream, RestReply};
use aws_wire::service::Call;
use bytes::Bytes;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::Inner;
use crate::answer::payload_of;
use crate::invoke::Input;
use crate::req::Req;

/// The body of the answer: what the driving task sends.
struct Frames(mpsc::Receiver<io::Result<Bytes>>);

impl ByteStream for Frames {
    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<io::Result<Bytes>>> {
        self.0.poll_recv(cx)
    }
}

impl Inner {
    pub(crate) async fn invoke_stream(self: &Arc<Self>, call: Call) -> Result<RestReply, ApiError> {
        let r = Req::new(&call);
        let payload = payload_of(&r)?;
        let rs = self.resolve(&call, &r, false).await?;
        let executed = rs.version.clone();
        if r.opt("InvocationType") == Some("DryRun") {
            return Ok(RestReply {
                json: json!({"StatusCode": 204}),
                ..RestReply::default()
            });
        }
        let input = Input {
            payload,
            client_context: None,
            trace_id: String::new(),
        };
        let started = match self.start(&rs, &call.caller, input).await? {
            Ok(s) => s,
            // A function that could not be launched answers as an error in the stream.
            Err(failed) => return Ok(self.failed_stream(&executed, &failed.payload)),
        };
        let (tx, rx) = mpsc::channel(16);
        let me = Arc::clone(self);
        let tail = r.opt("LogType") == Some("Tail");
        self.tasks.spawn(async move {
            let out = me.drive(started, Some(&tx)).await;
            let error = out.error.then(|| {
                (
                    "Unhandled",
                    String::from_utf8_lossy(&out.payload).into_owned(),
                )
            });
            let log = tail.then(|| aws_core::base64::encode(out.log.as_bytes()));
            let done = crate::eventstream::complete(
                error.as_ref().map(|(c, d)| (*c, d.as_str())),
                log.as_deref(),
            );
            drop(tx.send(Ok(Bytes::from(done))).await);
        });
        Ok(RestReply {
            json: stream_headers(&executed),
            status: None,
            body: Some(Box::pin(Frames(rx))),
        })
    }

    fn failed_stream(&self, executed: &str, payload: &Bytes) -> RestReply {
        let (tx, rx) = mpsc::channel(2);
        let msg = String::from_utf8_lossy(payload).into_owned();
        let done = crate::eventstream::complete(Some(("Unhandled", &msg)), None);
        drop(tx.try_send(Ok(Bytes::from(done))));
        RestReply {
            json: stream_headers(executed),
            status: None,
            body: Some(Box::pin(Frames(rx))),
        }
    }
}

fn stream_headers(executed: &str) -> Value {
    json!({"StatusCode": 200, "ExecutedVersion": executed, "ResponseStreamContentType": "application/vnd.amazon.eventstream"})
}
