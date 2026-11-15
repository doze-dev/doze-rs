//! IAM's local addresses (doze only, not AWS): `GET /_doze/iam/DozeAccessLog` (`?Reset=true` clears it) and
//! `GET /_doze/iam/DozeGeneratePolicy?Principal=<arn>&ScopeToResources=true` (see `accesslog`). IAM's own endpoint answers AWS's
//! actions alone: an `Action` outside the model is AWS's `InvalidAction`. Guarded by the gateway as every `/_doze/` address is.

use aws_peers::Mode;
use aws_wire::service::{BoxFuture, Web, WebReply, WebRequest};
use serde_json::{Value, json};

use crate::{Iam, Shared};

impl Shared {
    /// `/_doze/iam/DozeAccessLog` and `/_doze/iam/DozeGeneratePolicy`, with their parameters in the query string or, for a `POST`,
    /// a JSON object (`{"Principal": "...", "ScopeToResources": true}`). The gateway has already applied the local guard.
    fn doze(&self, request: &WebRequest) -> WebReply {
        let mut form = aws_wire::query::parse_form(&request.query);
        if let Ok(Value::Object(body)) = serde_json::from_slice::<Value>(&request.body) {
            form.extend(body.into_iter().map(|(k, v)| {
                let text = v.as_str().map_or_else(|| v.to_string(), str::to_owned);
                (k, text)
            }));
        }
        self.doze_action(request.path.trim_start_matches(DOZE_PATH), &form)
    }

    /// A doze-only action (`DozeAccessLog`, `DozeGeneratePolicy`), by its name and parameters. Served only at `/_doze/iam/<name>`:
    /// IAM's own endpoint answers AWS's actions alone.
    fn doze_action(
        &self,
        action: &str,
        form: &std::collections::BTreeMap<String, String>,
    ) -> WebReply {
        let yes = |k: &str| form.get(k).is_some_and(|v| v.eq_ignore_ascii_case("true"));
        let mode = match self.mode() {
            Mode::Off => "off",
            Mode::Soft => "soft",
            Mode::Enforce => "enforce",
        };
        let Ok(mut log) = self.log.lock() else {
            return web_reply(
                500,
                &json!({"Code": "InternalFailure", "Message": "The access log is unavailable."}),
            );
        };
        match action {
            "DozeAccessLog" => {
                if yes("Reset") {
                    *log = None;
                }
                let out = log.as_ref().map_or_else(
                    || json!({"Mode": mode, "Entries": [], "Evicted": 0}),
                    |l| l.entries(mode),
                );
                web_reply(200, &out)
            }
            "DozeGeneratePolicy" => {
                let principal = form.get("Principal").map(String::as_str);
                match log
                    .as_ref()
                    .and_then(|l| l.generate(principal, yes("ScopeToResources")))
                {
                    Some(out) => web_reply(200, &out),
                    None => {
                        let message = match (principal, mode) {
                            (_, "off") => "IAM is off, so nothing is recorded: run with --iam soft or enforce.".to_owned(),
                            (Some(p), _) => format!("No access has been recorded for principal {p}."),
                            (None, _) => "No access has been recorded yet.".to_owned(),
                        };
                        web_reply(404, &json!({"Code": "NoSuchEntity", "Message": message}))
                    }
                }
            }
            other => web_reply(
                404,
                &json!({"Code": "UnknownOperation", "Message": format!("doze has no IAM extension called {other:?}: there are DozeAccessLog and DozeGeneratePolicy.")}),
            ),
        }
    }
}

/// Where IAM's doze-only addresses live. `_` cannot begin a bucket name, so no S3 path-style address is taken.
const DOZE_PATH: &str = "/_doze/iam/";

fn web_reply(status: u16, body: &Value) -> WebReply {
    WebReply {
        status,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: body.to_string().into(),
    }
}

impl Web for Iam {
    fn owns(&self, _host: &str, path: &str) -> bool {
        path.starts_with(DOZE_PATH)
    }

    /// The access log names every principal and action seen: guarded as `/_doze/data` is.
    fn local(&self) -> bool {
        true
    }

    fn serve(&self, request: WebRequest) -> BoxFuture<'_, WebReply> {
        Box::pin(async move { self.0.doze(&request) })
    }
}
