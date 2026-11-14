//! Running a function once: resolve it, start or reuse a process, hand it the payload, wait for the answer under the function's
//! timeout, write its log. `Invoke` (all three types) is built on it; so are the event source pollers, the destinations and
//! function URLs.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aws_core::time::format_iso8601;
use aws_wire::ApiError;
use aws_wire::rest::RestReply;
use aws_wire::service::{Call, Caller};
use bytes::Bytes;
use serde_json::{Value, json};

use crate::Inner;
use crate::answer::{
    client_context_of, exit_document, init_document, join, launch_failure, payload_of, sync_reply,
    trace_of,
};
use crate::core::{Answer, Ask};
use crate::errors::{not_runnable, throttled};
use crate::livelog::Live;
use crate::req::Req;
use crate::resolve::{Resolved, Resolving};
use crate::rt::api::Chunk;
use crate::rt::logs::{Report, events};
use crate::rt::spec::Spec;
use crate::rt::{Ask as RunAsk, Event};

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
    /// What the invocation is counted under in `AWS/Lambda`.
    metric: crate::metrics::Target,
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
            key: format!(
                "{}/{}/{}/{}/{}",
                rs.region,
                rs.name,
                rs.version,
                rs.revision,
                self.local_stamp(rs)
            ),
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
        let metric = crate::metrics::Target::of(rs, caller);
        let running = match self.rt.begin(ask).await {
            Ok(r) => r,
            Err(why) => {
                self.count_invocation(&metric, crate::metrics::Outcome::not_started(why.is_none()))
                    .await;
                return match why {
                    None => Err(throttled(
                        "ReservedFunctionConcurrentInvocationLimitExceeded",
                    )),
                    Some(why) => Ok(Err(launch_failure(&why))),
                };
            }
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
            metric,
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
            metric,
        } = started;
        let id = running.id.clone();
        let mut out = RunOut {
            payload: Bytes::new(),
            error: false,
            request_id: id.clone(),
            content_type: None,
            log: String::new(),
        };
        let mut live = Live::new(
            running.tap(),
            &group,
            &caller,
            start_ms,
            &id,
            &version,
            json_logs,
            &name,
        );
        let waited = self.wait(&mut running, &mut out, stream, timeout);
        let (healthy, status) = self.with_live_log(&mut live, waited).await;
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
        let rows = events(
            start_ms,
            end_ms.max(start_ms),
            &live.all_lines(&fin.lines),
            &report,
        );
        out.log = rows.iter().map(|(_, m)| m.as_str()).collect();
        self.echo(&name, &fin.lines);
        let rest = live.unsent(rows);
        self.logger
            .write(self.peers.as_deref(), &caller, &group, &fin.stream, rest)
            .await;
        let ran = crate::metrics::Outcome::Ran {
            duration_ms: fin.duration_ms,
            error: out.error,
        };
        self.count_invocation(&metric, ran).await;
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
    pub(crate) fn echo(&self, name: &str, lines: &[(i64, String)]) {
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
