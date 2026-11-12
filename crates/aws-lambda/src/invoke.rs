//! Running a function once: resolve it, start or reuse a process, hand it the payload, wait for the answer under the function's
//! timeout, write its log. `Invoke` (all three types) is built on it; so are the event source pollers, the destinations and
//! function URLs.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aws_core::base64;
use aws_core::time::format_iso8601;
use aws_wire::ApiError;
use aws_wire::rest::RestReply;
use aws_wire::service::{Call, Caller};
use bytes::Bytes;
use serde_json::{Map, Value, json};

use crate::Inner;
use crate::core::{Answer, Ask};
use crate::errors::{invalid_content, not_runnable, throttled};
use crate::req::Req;
use crate::resolve::{Resolved, Resolving};
use crate::rt::api::Chunk;
use crate::rt::logs::{Report, events};
use crate::rt::spec::Spec;
use crate::rt::{Ask as RunAsk, Event, InitFailure};

/// What one invocation is given.
#[derive(Debug, Default)]
pub(crate) struct Input {
    pub(crate) payload: Bytes,
    pub(crate) client_context: Option<String>,
    pub(crate) trace_id: String,
}

/// An invocation a process has been given and that is not over.
#[derive(Debug)]
pub(crate) struct Started {
    running: crate::rt::Running,
    group: String,
    version: String,
    memory: i64,
    json_logs: bool,
    name: String,
    timeout: Duration,
    start_ms: i64,
    caller: Caller,
}

/// How one invocation ended.
#[derive(Debug)]
pub(crate) struct RunOut {
    pub(crate) payload: Bytes,
    pub(crate) error: bool,
    pub(crate) request_id: String,
    pub(crate) content_type: Option<String>,
    /// The log of the invocation as text, for `LogType: Tail`.
    pub(crate) log: String,
}

/// Jackson's complaint about a payload that is not JSON, as AWS words it (recorded 2026-11-09 for a key that is not quoted).
fn parse_failure(payload: &[u8]) -> ApiError {
    let text = String::from_utf8_lossy(payload);
    let detail = match serde_json::from_str::<Value>(&text) {
        Ok(_) => return invalid_content("Could not parse request body into json."),
        Err(e) => {
            let first = text.trim_start().chars().next();
            let bad = text
                .chars()
                .nth(e.column().saturating_sub(1))
                .unwrap_or('?');
            if first == Some('{') && bad != '"' {
                format!(
                    "Unexpected character ('{bad}' (code {})): was expecting double-quote to start field name\n at [Source: REDACTED (`StreamReadFeature.INCLUDE_SOURCE_IN_LOCATION` disabled); line: {}, column: {}]",
                    u32::from(bad),
                    e.line(),
                    e.column()
                )
            } else {
                format!("{e}")
            }
        }
    };
    invalid_content(format!(
        "Could not parse request body into json: Could not parse payload into json: {detail}"
    ))
}

/// The payload of an invoke request: absent is `{}`; what is there must be JSON.
pub(crate) fn payload_of(r: &Req) -> Result<Bytes, ApiError> {
    let bytes = r
        .opt("Payload")
        .and_then(base64::decode)
        .unwrap_or_default();
    if bytes.is_empty() {
        return Ok(Bytes::from_static(b"{}"));
    }
    if serde_json::from_slice::<Value>(&bytes).is_err() {
        return Err(parse_failure(&bytes));
    }
    Ok(Bytes::from(bytes))
}

fn client_context_of(r: &Req) -> Option<String> {
    let raw = r.opt("ClientContext")?;
    let bytes = base64::decode(raw)?;
    String::from_utf8(bytes).ok()
}

impl Inner {
    /// Finds what is needed to run the function a request names, as the actor sees it.
    pub(crate) async fn resolve(
        &self,
        call: &Call,
        r: &Req,
        url: bool,
    ) -> Result<Box<Resolved>, ApiError> {
        let f = r.function()?;
        let draw = aws_seal::random_below(1_000_000).map_or(0.0, |n| n as f64 / 1_000_000.0);
        let ask = Resolving {
            caller: call.caller.clone(),
            function: f.name.clone(),
            qualifier: f.qualifier,
            source_arn: r.opt("SourceArn").map(str::to_owned),
            draw,
            url,
            internal: false,
        };
        match self
            .ask(&call.caller.region, Ask::Resolve(Box::new(ask)))
            .await?
        {
            Answer::Resolved(done) => done,
            _ => Err(ApiError::internal(
                "Lambda answered a resolution with something else",
            )),
        }
    }

    /// The launch specification for a resolved function.
    fn spec(
        &self,
        rs: &Resolved,
        caller: &Caller,
        creds: crate::rt::spec::Creds,
    ) -> Result<Spec, ApiError> {
        if rs.code.image.is_some() {
            return Err(not_runnable(
                "Container-image functions cannot run on doze-aws: it runs functions as host processes and pulls no images. Use a zip package, or set Command to the launch line of a process that speaks the Runtime API.",
            ));
        }
        let text = |k: &str| {
            rs.config
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let vars = rs
            .config
            .get("Environment")
            .and_then(|e| e.get("Variables"))
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        let logging = rs.config.get("LoggingConfig");
        let command = rs.config.get("Command").and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        });
        Ok(Spec {
            name: rs.name.clone(),
            version: rs.version.clone(),
            region: rs.region.clone(),
            runtime: text("Runtime"),
            handler: text("Handler"),
            command,
            root: rs.root.clone(),
            vars,
            memory: rs
                .config
                .get("MemorySize")
                .and_then(Value::as_i64)
                .unwrap_or(128),
            layers: self.layer_paths(rs),
            log_group: logging
                .and_then(|l| l.get("LogGroup"))
                .and_then(Value::as_str)
                .map_or_else(|| format!("/aws/lambda/{}", rs.name), str::to_owned),
            log_stream: String::new(),
            endpoint: self.endpoint_for(caller),
            shims: PathBuf::new(),
            creds,
            interpreters: Default::default(),
            json_logs: logging
                .and_then(|l| l.get("LogFormat"))
                .and_then(Value::as_str)
                == Some("JSON"),
        })
    }

    /// The layers that are kept here; one that is not is said in the log and the function runs without it.
    fn layer_paths(&self, rs: &Resolved) -> Vec<PathBuf> {
        rs.layers
            .iter()
            .filter_map(|l| match l {
                Ok(p) => Some(p.clone()),
                Err(arn) => {
                    tracing::warn!(function = %rs.name, "layer {arn} is not available locally; the function runs without it");
                    None
                }
            })
            .collect()
    }

    fn timeout_of(rs: &Resolved) -> Duration {
        Duration::from_secs(
            u64::try_from(
                rs.config
                    .get("Timeout")
                    .and_then(Value::as_i64)
                    .unwrap_or(3),
            )
            .unwrap_or(3)
            .max(1),
        )
    }

    /// Runs the function once and waits for its answer.
    ///
    /// # Errors
    /// Reserved concurrency is used up, or the function cannot run here (an image).
    pub(crate) async fn run_function(
        &self,
        rs: &Resolved,
        caller: &Caller,
        input: Input,
    ) -> Result<RunOut, ApiError> {
        match self.start(rs, caller, input).await? {
            Ok(started) => Ok(self.drive(started, None).await),
            Err(failed) => Ok(failed),
        }
    }

    /// Hands the invocation to a process. `Ok(Err(_))` is an invocation that failed before it began (the process could not be
    /// launched), answered like any other function error.
    pub(crate) async fn start(
        &self,
        rs: &Resolved,
        caller: &Caller,
        input: Input,
    ) -> Result<Result<Started, RunOut>, ApiError> {
        let role = rs
            .config
            .get("Role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let creds = self
            .roles
            .credentials(
                self.peers.as_deref(),
                caller,
                role,
                &rs.name,
                self.clock.now_ms(),
            )
            .await;
        let spec = self.spec(rs, caller, creds)?;
        let (group, version, memory, json_logs) = (
            spec.log_group.clone(),
            rs.version.clone(),
            spec.memory,
            spec.json_logs,
        );
        let ask = RunAsk {
            key: format!("{}/{}/{}/{}", rs.region, rs.name, rs.version, rs.revision),
            function: format!("{}/{}", rs.region, rs.name),
            reserved: rs.reserved,
            spec,
            payload: input.payload,
            timeout: Self::timeout_of(rs),
            invoked_arn: rs.invoked_arn.clone(),
            client_context: input.client_context,
            trace_id: input.trace_id,
        };
        let (timeout, start_ms) = (ask.timeout, self.clock.now_ms());
        let running = match self.rt.begin(ask).await {
            Ok(r) => r,
            Err(None) => {
                return Err(throttled(
                    "ReservedFunctionConcurrentInvocationLimitExceeded",
                ));
            }
            Err(Some(why)) => return Ok(Err(launch_failure(&why))),
        };
        Ok(Ok(Started {
            running,
            group,
            version,
            memory,
            json_logs,
            name: rs.name.clone(),
            timeout,
            start_ms,
            caller: caller.clone(),
        }))
    }

    /// Waits for the invocation to end, and writes its log. With `stream`, the response is sent on as it arrives (framed as an event
    /// stream) instead of being kept.
    pub(crate) async fn drive(
        &self,
        started: Started,
        stream: Option<&tokio::sync::mpsc::Sender<std::io::Result<Bytes>>>,
    ) -> RunOut {
        let Started {
            mut running,
            group,
            version,
            memory,
            json_logs,
            name,
            timeout,
            start_ms,
            caller,
        } = started;
        let id = running.id.clone();
        let mut out = RunOut {
            payload: Bytes::new(),
            error: false,
            request_id: id.clone(),
            content_type: None,
            log: String::new(),
        };
        let (healthy, status) = self.wait(&mut running, &mut out, stream, timeout).await;
        let end_ms = self.clock.now_ms();
        let fin = running.finish(healthy, &self.rt).await;
        let report = Report {
            id: &id,
            version: &version,
            duration_ms: fin.duration_ms,
            init_ms: fin.init_ms,
            memory,
            status,
            json: json_logs,
        };
        let rows = events(start_ms, end_ms.max(start_ms), &fin.lines, &report);
        out.log = rows.iter().map(|(_, m)| m.as_str()).collect();
        self.echo(&name, &fin.lines);
        self.logger
            .write(self.peers.as_deref(), &caller, &group, &fin.stream, rows)
            .await;
        out
    }

    /// Waits for the process to answer, takes what it says into `out`, and reports whether the process is still good and how the
    /// invocation ended if it did not succeed.
    async fn wait(
        &self,
        running: &mut crate::rt::Running,
        out: &mut RunOut,
        stream: Option<&tokio::sync::mpsc::Sender<std::io::Result<Bytes>>>,
        timeout: Duration,
    ) -> (bool, Option<&'static str>) {
        let id = running.id.clone();
        if let Err(failure) = running.started().await {
            out.error = true;
            out.payload = init_document(&failure, &id, timeout);
            return (false, Some("error"));
        }
        loop {
            match running.next().await {
                Event::Chunk(Chunk::Type(t)) => out.content_type = Some(t),
                Event::Chunk(Chunk::Data(b)) => match stream {
                    Some(tx) => drop(
                        tx.send(Ok(Bytes::from(crate::eventstream::chunk(&b))))
                            .await,
                    ),
                    None => out.payload = join(&out.payload, &b),
                },
                Event::Chunk(Chunk::End) => return (true, None),
                Event::Chunk(Chunk::Fail(body)) => {
                    (out.error, out.payload) = (true, body);
                    return (true, Some("error"));
                }
                Event::Timeout => {
                    out.error = true;
                    let at = format_iso8601(self.clock.now_ms() / 1000);
                    out.payload = crate::rt::timeout_document(&id, &at, timeout);
                    return (false, Some("timeout"));
                }
                Event::Crashed => {
                    out.error = true;
                    out.payload = exit_document(&id, running.exit_status().as_deref());
                    return (false, Some("error"));
                }
            }
        }
    }

    /// Prints what the function wrote to our own log, unless it is quiet.
    fn echo(&self, name: &str, lines: &[(i64, String)]) {
        if self.config.quiet {
            return;
        }
        for (_, line) in lines {
            tracing::info!("lambda[{name}] {line}");
        }
    }

    /// `Invoke`.
    pub(crate) async fn invoke(self: &Arc<Self>, call: Call) -> Result<RestReply, ApiError> {
        let r = Req::new(&call);
        let kind = r
            .opt("InvocationType")
            .unwrap_or("RequestResponse")
            .to_owned();
        let payload = payload_of(&r)?;
        let rs = self.resolve(&call, &r, false).await?;
        let executed = rs.version.clone();
        match kind.as_str() {
            "DryRun" => Ok(RestReply {
                json: json!({"StatusCode": 204}),
                ..RestReply::default()
            }),
            "Event" => {
                self.async_invocation(rs, call.caller.clone(), payload, client_context_of(&r));
                Ok(RestReply {
                    json: json!({"StatusCode": 202}),
                    ..RestReply::default()
                })
            }
            _ => {
                let input = Input {
                    payload,
                    client_context: client_context_of(&r),
                    trace_id: trace_of(&call),
                };
                let out = self.run_function(&rs, &call.caller, input).await?;
                let tail = r.opt("LogType") == Some("Tail");
                Ok(RestReply {
                    json: sync_reply(&out, &executed, tail),
                    ..RestReply::default()
                })
            }
        }
    }
}

/// The answer of a synchronous invoke.
fn sync_reply(out: &RunOut, executed: &str, tail: bool) -> Value {
    let mut reply = Map::new();
    reply.insert("StatusCode".into(), json!(200));
    reply.insert("ExecutedVersion".into(), json!(executed));
    if out.error {
        reply.insert("FunctionError".into(), json!("Unhandled"));
    }
    if tail {
        let bytes = out.log.as_bytes();
        let from = bytes.len().saturating_sub(4096);
        let cut = bytes.get(from..).unwrap_or(bytes);
        reply.insert("LogResult".into(), json!(base64::encode(cut)));
    }
    reply.insert("Payload".into(), json!(base64::encode(&out.payload)));
    Value::Object(reply)
}

fn trace_of(call: &Call) -> String {
    let id = call.id.as_str().replace('-', "");
    let root: String = id.chars().take(24).collect();
    format!(
        "Root=1-{}-{root};Sampled=0",
        id.chars().take(8).collect::<String>()
    )
}

fn join(a: &Bytes, b: &Bytes) -> Bytes {
    if a.is_empty() {
        return b.clone();
    }
    let mut out = Vec::with_capacity(a.len() + b.len());
    out.extend_from_slice(a);
    out.extend_from_slice(b);
    Bytes::from(out)
}

/// A process that could not be started is a function error, as a broken init is on AWS: a 200 with the reason in the payload.
fn launch_failure(why: &str) -> RunOut {
    RunOut {
        payload: Bytes::from(
            json!({"errorMessage": why, "errorType": "Runtime.LaunchError"}).to_string(),
        ),
        error: true,
        request_id: aws_seal::uuid_v4().unwrap_or_default(),
        content_type: None,
        log: String::new(),
    }
}

fn init_document(failure: &InitFailure, id: &str, timeout: Duration) -> Bytes {
    match failure {
        InitFailure::Timeout => Bytes::from(json!({"errorMessage": format!("RequestId: {id} Error: Task timed out during init after 10.00 seconds"), "errorType": "Runtime.InitTimeout"}).to_string()),
        InitFailure::Exited(Some(reported)) => reported.clone(),
        InitFailure::Exited(None) => {
            let _ = timeout;
            exit_document(id, None)
        }
    }
}

fn exit_document(id: &str, status: Option<&str>) -> Bytes {
    let how = status.map_or_else(
        || "exit status 1".to_owned(),
        |s| s.replace("exit status: ", "exit status "),
    );
    Bytes::from(json!({"errorMessage": format!("RequestId: {id} Error: Runtime exited with error: {how}"), "errorType": "Runtime.ExitError"}).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_payload_that_is_not_json_is_refused_in_jacksons_words() {
        let e = parse_failure(b"{not json");
        assert_eq!(e.code, "InvalidRequestContentException");
        assert!(
            e.message.starts_with("Could not parse request body into json: Could not parse payload into json: Unexpected character ('n' (code 110)): was expecting double-quote to start field name\n at [Source: REDACTED (`StreamReadFeature.INCLUDE_SOURCE_IN_LOCATION` disabled); line: 1, column: 2]"),
            "{}",
            e.message
        );
    }
}
