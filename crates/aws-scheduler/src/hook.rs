//! The local hook (doze only, not AWS): `POST /_doze/scheduler/<region>/<group>/<name>/fire[?account=<id>]` invokes a schedule now,
//! so a test need not wait for its time. The answer says what the first attempt came to; a failure then goes on through the target's
//! retry policy and dead-letter queue as a scheduled invocation would. A one-time schedule's fire through the hook is its one fire
//! (it completes, and is deleted if `ActionAfterCompletion` is `DELETE`); a recurring schedule keeps its plan. Unsigned, like every
//! `/_doze/` address: it is for the developer's own machine.

use std::sync::Arc;

use aws_wire::service::{WebReply, WebRequest};
use serde_json::{Value, json};

use crate::core::Request;
use crate::deliver::Outcome;
use crate::{Scheduler, deliver, retry};

/// Where the hook lives. `_` cannot begin a bucket name, so no S3 path-style address is taken.
pub(crate) const PATH: &str = "/_doze/scheduler/";

pub(crate) async fn serve(s: &Scheduler, request: WebRequest) -> WebReply {
    let rest = request.path.strip_prefix(PATH).unwrap_or_default();
    let parts: Vec<&str> = rest.split('/').collect();
    let [region, group, name, "fire"] = parts.as_slice() else {
        return reply(
            404,
            &json!({"Code": "UnknownOperation", "Message": format!("doze has no Scheduler hook at {}: use POST {PATH}<region>/<group>/<name>/fire.", request.path)}),
        );
    };
    if request.method != "POST" {
        return reply(
            405,
            &json!({"Code": "MethodNotAllowed", "Message": "Fire a schedule with POST."}),
        );
    }
    let account = aws_wire::query::parse_form(&request.query)
        .get("account")
        .cloned();
    let decode = |t: &str| {
        aws_core::uri::decode_str(t).map_or_else(|| t.to_owned(), std::borrow::Cow::into_owned)
    };
    let ask = Request::Fire {
        account,
        group: decode(group),
        name: decode(name),
    };
    let (answer, fired) = match s.ask(region, ask).await {
        Ok(got) => got,
        Err(e) => return error(&e),
    };
    if let Err(e) = answer {
        return error(&e);
    }
    let mut out = Vec::new();
    for inv in fired {
        let first = deliver::attempt(&s.shared, &inv, 1).await;
        out.push(describe(&inv, &first));
        let sh = Arc::clone(&s.shared);
        s.shared
            .tasks
            .spawn(async move { retry::carry_on(sh, inv, 1, first).await });
    }
    reply(200, out.pop().as_ref().unwrap_or(&Value::Null))
}

fn describe(inv: &crate::fire::Invocation, outcome: &Outcome) -> Value {
    let (state, error) = match outcome {
        Outcome::Sent => ("DELIVERED", Value::Null),
        Outcome::NotLocal(why) => ("NOT_DELIVERED", json!({"Message": why})),
        Outcome::Failed(f) => (
            "FAILED",
            json!({"Code": f.code, "Message": f.message, "Retryable": f.retryable}),
        ),
    };
    json!({
        "ScheduleArn": inv.schedule_arn,
        "ExecutionId": inv.execution_id,
        "ScheduledTime": aws_core::time::format_iso8601(inv.scheduled_ms.div_euclid(1000)).replace(".000Z", "Z"),
        "TargetArn": inv.target.get("Arn"),
        "Outcome": state,
        "Error": error,
    })
}

fn error(e: &aws_wire::ApiError) -> WebReply {
    let status = if e.code == "ResourceNotFoundException" {
        404
    } else {
        e.status
    };
    reply(status, &json!({"Code": e.code, "Message": e.message}))
}

fn reply(status: u16, body: &Value) -> WebReply {
    WebReply {
        status,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: body.to_string().into(),
    }
}
