//! Shared by the registration files: decoding a call's arguments, and the route closure every registration builds.

use doze_module_proto::peer::Incoming;

/// The call's arguments, or the call answered with why they do not decode.
pub(crate) fn args<T: for<'b> minicbor::Decode<'b, ()>>(call: Incoming) -> Option<(T, Incoming)> {
    match call.args::<T>() {
        Ok(a) => Some((a, call)),
        Err(e) => {
            let _ = call.responder.err(&e);
            None
        }
    }
}

/// A route: an async closure over the module, the call and the peer, typed for `Server<M>`.
#[macro_export]
#[doc(hidden)]
macro_rules! route {
    (|$m:ident, $call:ident, $peer:ident| $body:expr) => {{
        let r: Route<M> = Arc::new(
            move |$m: Arc<M>, $call: Incoming, $peer: doze_module_proto::peer::Peer| {
                Box::pin(async move { $body })
            },
        );
        r
    }};
}
