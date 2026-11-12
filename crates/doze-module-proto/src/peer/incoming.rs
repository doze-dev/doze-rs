//! Incoming calls: the guards of the method table, then a task for the handler.

use super::serve::{Cancel, Incoming, Responder};
use super::{Handler, Inner, Out, Side};
use crate::flow::Outbound;
use crate::frame::{self, Frame, Kind};
use crate::method::{Direction, Method, Shape};
use crate::msg::error::{Error, ErrorKind};
use crate::msg::{self, CallHead};
use doze_os::channel::Reader;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

/// Refuses a call before any handler sees it.
fn refuse(inner: &Arc<Inner>, id: u32, e: &Error) -> Result<(), Error> {
    let body = msg::encode(e).map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?;
    let bytes = frame::encode(id, Kind::Error, 0, &body, None)
        .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?;
    inner.queue(Out::Frame(bytes))
}

/// Why an incoming call is not allowed, if it is not.
fn guard(inner: &Arc<Inner>, method: Option<Method>, raw: u16, head: &CallHead) -> Option<Error> {
    let unsupported = |m: String| Some(Error::new(ErrorKind::Unsupported, m));
    let Some(m) = method else {
        return unsupported(format!("method {raw} is not known to this side"));
    };
    let theirs = match inner.side {
        Side::Host => Direction::ModuleToHost,
        Side::Module => Direction::HostToModule,
    };
    if m.direction() != theirs {
        return unsupported(format!("{m:?} is not called from that side"));
    }
    if m.capability() != 0 && inner.caps.load(Ordering::SeqCst) & m.capability() == 0 {
        return unsupported(format!(
            "{m:?} needs a capability the module did not declare"
        ));
    }
    if let Some(scope) = m.scope()
        && !head.for_call.is_some_and(|id| inner.in_flight(id, scope))
    {
        return Some(Error::new(
            ErrorKind::Invalid,
            format!("{m:?} is only allowed while a {scope:?} call is in flight"),
        ));
    }
    None
}

pub(super) fn incoming(
    inner: &Arc<Inner>,
    r: &mut Reader,
    handler: &Handler,
    f: Frame,
) -> Result<(), Error> {
    let handle = match f.handle {
        Some(t) => Some(
            r.import(&t)
                .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?,
        ),
        None => None,
    };
    let Ok((head, args)) = msg::decode_call(&f.body) else {
        return refuse(
            inner,
            f.id,
            &Error::new(ErrorKind::Invalid, "a call body that does not decode"),
        );
    };
    let method = Method::from_u16(f.method);
    if let Some(e) = guard(inner, method, f.method, &head) {
        return refuse(inner, f.id, &e);
    }
    let Some(method) = method else { return Ok(()) };
    let cancel = Cancel::default();
    if let Some(ms) = head.deadline_ms {
        let c = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(u64::from(ms))).await;
            c.fire();
        });
    }
    inner
        .serving
        .lock()
        .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?
        .insert(f.id, cancel.clone());
    let credit = (method.shape() == Shape::Stream)
        .then(|| Arc::new((Mutex::new(Outbound::new()), Notify::new())));
    if let Some(c) = &credit {
        inner
            .credits
            .lock()
            .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?
            .insert(f.id, c.clone());
    }
    let responder = Responder {
        id: f.id,
        inner: inner.clone(),
        credit,
        answered: false,
    };
    let call = Incoming {
        method,
        head,
        args: args.to_vec(),
        handle,
        responder,
        cancel,
    };
    tokio::spawn(handler(call));
    Ok(())
}
