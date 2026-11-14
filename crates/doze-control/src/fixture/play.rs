//! Playing a stream: the acknowledgement, the script's frames that the stream asked for, then (with `hold`) whatever
//! is pushed until it is cancelled, and the end frame. The filter is Go fakedaemon's `Subscribe`.

use super::session::{Writer, send};
use super::{Live, Msg, State, Step};
use crate::client::connection::lock;
use crate::client::wait::block_on;
use crate::{Frame, Op, Request, Response, Topic, parse_go_duration};
use async_channel::Receiver;
use std::sync::Arc;
use std::time::Instant;

/// What a stream asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Filter {
    topics: Vec<String>,
    names: Vec<String>,
    panel: String,
    selector: String,
    /// A `subscribe` (rather than `events` or a logs follow).
    subscribe: bool,
}

impl Filter {
    /// The filter a streaming request asks for, or the error Go's server answers it with.
    fn of(req: &Request) -> Result<Filter, String> {
        match req.known_op() {
            Some(Op::Events) => Ok(Filter {
                topics: vec![Topic::Instances.as_str().into()],
                ..Filter::default()
            }),
            Some(Op::Logs) => {
                let names = if req.names.is_empty() && !req.db.is_empty() {
                    vec![req.db.clone()]
                } else {
                    req.names.clone()
                };
                Ok(Filter {
                    topics: vec![Topic::Logs.as_str().into()],
                    names,
                    ..Filter::default()
                })
            }
            _ => {
                if !req.interval.is_empty()
                    && parse_go_duration(&req.interval).is_none_or(|d| d.is_zero())
                {
                    return Err(format!("subscribe: {:?} is not an interval", req.interval));
                }
                if req.topics.is_empty() {
                    return Err("subscribe: name at least one topic".into());
                }
                Ok(Filter {
                    topics: req.topics.clone(),
                    names: req.names.clone(),
                    panel: req.panel.clone(),
                    selector: req.selector.clone(),
                    subscribe: true,
                })
            }
        }
    }

    /// Whether a frame goes to this stream.
    pub(crate) fn accepts(&self, f: &Frame) -> bool {
        if !self.topics.contains(&f.topic) {
            return false;
        }
        let named = |n: &String| self.names.is_empty() || self.names.contains(n);
        // A subscriber that named instances is sent the lines of those.
        if f.log.as_ref().is_some_and(|l| !named(&l.instance)) {
            return false;
        }
        // A console follows the one instance named; a stream row goes only to the panel asked for and, where the row
        // says which ("_for"), that row of its table.
        if let Some(c) = &f.console {
            if !c.instance.is_empty() && !named(&c.instance) {
                return false;
            }
            if let Some(row) = &c.row {
                let wanted_for = row.cell("_for").filter(|s| !s.is_empty());
                if self.panel != c.panel || wanted_for.is_some_and(|f| f != self.selector) {
                    return false;
                }
            }
        }
        true
    }
}

/// Runs one stream to its end.
pub(crate) fn run(state: &Arc<State>, conn: usize, w: &Writer, req: &Request) {
    let filter = match Filter::of(req) {
        Ok(f) => f,
        Err(error) => {
            let _ = send(
                w,
                &Frame {
                    id: req.id,
                    response: Some(Response {
                        error,
                        ..Response::default()
                    }),
                    ..Frame::default()
                },
            );
            return;
        }
    };
    let (tx, rx) = async_channel::unbounded();
    let key = (conn, req.id);
    lock(&state.streams).insert(
        key,
        Live {
            tx,
            filter: filter.clone(),
            held: false,
        },
    );
    let end = play(state, key, w, &filter, &rx);
    lock(&state.streams).remove(&key);
    if let Some(error) = end {
        let _ = send(
            w,
            &Frame {
                id: req.id,
                end: true,
                error: error.unwrap_or_default(),
                ..Frame::default()
            },
        );
    }
}

/// Plays the stream. `None` when the connection is gone; otherwise the end, with its error if it failed.
fn play(
    state: &State,
    key: (usize, u64),
    w: &Writer,
    filter: &Filter,
    rx: &Receiver<Msg>,
) -> Option<Option<String>> {
    let id = key.1;
    let ack = Response {
        ok: true,
        ..Response::default()
    };
    send(
        w,
        &Frame {
            id,
            response: Some(ack),
            ..Frame::default()
        },
    )
    .ok()?;
    let (steps, hold, instances) = {
        let s = lock(&state.script);
        (s.steps.clone(), s.hold, s.status.instances.clone())
    };
    // The daemon opens an instances subscription with a snapshot of every instance.
    if filter.subscribe && filter.topics.iter().any(|t| t == Topic::Instances.as_str()) {
        let topic = Topic::Instances.as_str().to_owned();
        send(
            w,
            &Frame {
                id,
                topic,
                snapshot: true,
                instances,
                ..Frame::default()
            },
        )
        .ok()?;
    }
    for step in steps {
        let msg = match step {
            Step::Wait(d) => block_on(rx.recv(), Some(Instant::now() + d)).and_then(Result::ok),
            Step::Frame(f) => {
                if filter.accepts(&f) {
                    send(w, &Frame { id, ..*f }).ok()?;
                }
                rx.try_recv().ok()
            }
        };
        if let Some(Msg::End(e)) = msg {
            return Some(e);
        }
    }
    if !hold {
        return Some(None);
    }
    if let Some(live) = lock(&state.streams).get_mut(&key) {
        live.held = true;
    }
    loop {
        match rx.recv_blocking() {
            Ok(Msg::Frame(f)) => send(w, &Frame { id, ..*f }).ok()?,
            Ok(Msg::End(e)) => return Some(e),
            Err(_) => return Some(None),
        }
    }
}
