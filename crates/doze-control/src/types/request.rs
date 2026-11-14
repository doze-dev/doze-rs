//! What a client sends (Go `Request`), and the builders for each op.

use super::ops::{LogQuery, Op, Subscription, Verb};
use crate::json::{is_false, is_zero_i64, is_zero_u64};
use crate::time::Timestamp;
use crate::{Hello, PROTO_VERSION, format_go_duration};
use serde::{Deserialize, Serialize};

/// A command sent from a client to the daemon (Go `Request`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
    /// Opens a connection; it is the whole of the first frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hello: Option<Hello>,
    /// Chosen by the client and echoed on every frame that answers this request.
    #[serde(skip_serializing_if = "is_zero_u64")]
    pub id: u64,
    /// See [`Op`].
    #[serde(skip_serializing_if = "String::is_empty")]
    pub op: String,
    /// The target instance (empty = all, where meaningful).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub db: String,
    /// `admin`: the action to run.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub action: String,
    /// `admin`: the resource it runs on.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub resource: String,
    /// `admin`: its input.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub input: String,
    /// Turns `logs` into a held stream of log frames.
    #[serde(skip_serializing_if = "is_false")]
    pub follow: bool,
    /// Scopes a `logs` follow, or the `logs` and `console` topics, to these instances.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    /// `add`: the rendered HCL of one instance.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub block: String,
    /// `remove`: also delete the instance's data directory.
    #[serde(skip_serializing_if = "is_false")]
    pub wipe: bool,
    /// `subscribe`: the topics.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<String>,
    /// `subscribe`: the journal sequence number to resume after.
    #[serde(skip_serializing_if = "is_zero_u64")]
    pub since: u64,
    /// `subscribe`: the stats period, a Go duration (`"1s"`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub interval: String,
    /// `subscribe`: recent log lines of each instance to send before following.
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub backlog: i64,
    /// `subscribe`: the console stream panel to run.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub panel: String,
    /// `subscribe`: the row of the panel's table the stream follows.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub selector: String,
    /// `logs`: which run (0 newest, -1 the one before, a positive run id).
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub run: i64,
    /// `logs`: how many lines from the end.
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub tail: i64,
    /// `logs`: drop lines older than this. Go's `omitzero`.
    #[serde(skip_serializing_if = "Timestamp::is_zero")]
    pub since_time: Timestamp,
}

impl Request {
    /// The first frame of a connection.
    pub fn hello(client: &str) -> Request {
        Request {
            hello: Some(Hello {
                proto: PROTO_VERSION,
                client: client.to_owned(),
            }),
            ..Request::default()
        }
    }

    /// A bare op with no target.
    pub fn op(op: Op) -> Request {
        Request {
            op: op.as_str().to_owned(),
            ..Request::default()
        }
    }

    /// An op on one instance.
    pub fn on(op: Op, db: &str) -> Request {
        Request {
            db: db.to_owned(),
            ..Request::op(op)
        }
    }

    /// A verb on its target (an empty target is the whole stack, where meaningful).
    pub fn verb(verb: Verb, target: &str) -> Request {
        Request::on(verb.op(), target)
    }

    /// Stop the request `id` (a stream).
    pub fn cancel(id: u64) -> Request {
        Request {
            id,
            ..Request::op(Op::Cancel)
        }
    }

    /// Open a subscription.
    pub fn subscribe(sub: &Subscription) -> Request {
        Request {
            topics: sub.topics.iter().map(|t| t.as_str().to_owned()).collect(),
            names: sub.names.clone(),
            since: sub.since,
            interval: sub
                .interval
                .filter(|d| !d.is_zero())
                .map(format_go_duration)
                .unwrap_or_default(),
            backlog: sub.backlog,
            panel: sub.panel.clone(),
            selector: sub.selector.clone(),
            ..Request::op(Op::Subscribe)
        }
    }

    /// Read lines an instance printed.
    pub fn logs(q: &LogQuery) -> Request {
        Request {
            run: q.run,
            tail: q.tail,
            since_time: q.since.clone(),
            ..Request::on(Op::Logs, &q.instance)
        }
    }

    /// Follow the logs of `names` (empty = every running backend).
    pub fn follow_logs(names: &[String]) -> Request {
        Request {
            follow: true,
            names: names.to_vec(),
            ..Request::op(Op::Logs)
        }
    }

    /// Run a module's data action on a resource of it.
    pub fn admin(instance: &str, action: &str, resource: &str, input: &str) -> Request {
        Request {
            action: action.to_owned(),
            resource: resource.to_owned(),
            input: input.to_owned(),
            ..Request::on(Op::Admin, instance)
        }
    }

    /// Wire one rendered HCL block into the running stack.
    pub fn add(block: &str) -> Request {
        Request {
            block: block.to_owned(),
            ..Request::op(Op::Add)
        }
    }

    /// Tear an instance down, and optionally wipe its data.
    pub fn remove(name: &str, wipe: bool) -> Request {
        Request {
            wipe,
            ..Request::on(Op::Remove, name)
        }
    }

    /// The op, if it is one this protocol knows.
    pub fn known_op(&self) -> Option<Op> {
        Op::parse(&self.op)
    }

    /// Whether the daemon answers this request with a stream rather than one response.
    pub fn is_stream(&self) -> bool {
        match self.known_op() {
            Some(Op::Subscribe | Op::Events) => true,
            Some(Op::Logs) => self.follow,
            _ => false,
        }
    }
}
