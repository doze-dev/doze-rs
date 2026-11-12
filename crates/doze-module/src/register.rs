//! Registering capabilities on the server. Each method needs its trait, sets its capability bit, and routes its calls.

use crate::caps::{
    Admin, Attributes, BackendUrl, Console, Converge, Health, Objects, Prune, Restarts, Rows,
    Template,
};
use crate::module::Module;
use crate::server::{Route, Server};
use crate::with_cx;
use doze_module_proto::method::Method;
use doze_module_proto::msg::calls::{PruneArgs, RunAction, TemplateClone, TemplateEnsure, Unit};
use doze_module_proto::msg::console::TailArgs;
use doze_module_proto::msg::error::{Error, ErrorKind};
use doze_module_proto::msg::hello::caps;
use doze_module_proto::msg::instance::Instance;
use doze_module_proto::msg::run::WithToolchain;
use doze_module_proto::peer::Incoming;
use std::sync::Arc;

fn args<T: for<'b> minicbor::Decode<'b, ()>>(call: Incoming) -> Option<(T, Incoming)> {
    match call.args::<T>() {
        Ok(a) => Some((a, call)),
        Err(e) => {
            let _ = call.responder.err(&e);
            None
        }
    }
}

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

impl<M: Module> Server<M> {
    pub(crate) fn add(mut self, bits: u64, method: Method, r: Route<M>) -> Self {
        self.caps |= bits;
        self.routes.insert(method, r);
        self
    }

    pub fn converge(self) -> Self
    where
        M: Converge,
    {
        self.add(
            caps::CONVERGE,
            Method::Converge,
            route!(|m, call, peer| {
                let Some((a, call)) = args::<WithToolchain>(call) else {
                    return;
                };
                with_cx!(m, a.instance, call, peer, |cx| m
                    .converge(&cx, &a.toolchain)
                    .await
                    .map(|()| Unit {}));
            }),
        )
    }

    pub fn objects(self) -> Self
    where
        M: Objects,
    {
        self.add(
            caps::OBJECTS,
            Method::Objects,
            route!(|m, call, peer| {
                let Some((inst, call)) = args::<Instance>(call) else {
                    return;
                };
                with_cx!(m, inst, call, peer, |cx| m.objects(&cx));
            }),
        )
    }

    pub fn prune(self) -> Self
    where
        M: Prune,
    {
        self.add(
            caps::PRUNE,
            Method::Prune,
            route!(|m, call, peer| {
                let Some((a, call)) = args::<PruneArgs>(call) else {
                    return;
                };
                with_cx!(m, a.instance, call, peer, |cx| m
                    .prune(&cx, &a.toolchain, &a.removed)
                    .await
                    .map(|()| Unit {}));
            }),
        )
    }

    pub fn attributes(self) -> Self
    where
        M: Attributes,
    {
        self.add(
            caps::ATTRIBUTES,
            Method::Attributes,
            route!(|m, call, peer| {
                let Some((inst, call)) = args::<Instance>(call) else {
                    return;
                };
                with_cx!(m, inst, call, peer, |cx| m.attributes(&cx));
            }),
        )
    }

    pub fn backend_url(self) -> Self
    where
        M: BackendUrl,
    {
        self.add(
            caps::BACKEND_URL,
            Method::BackendUrl,
            route!(|m, call, peer| {
                let Some((inst, call)) = args::<Instance>(call) else {
                    return;
                };
                with_cx!(m, inst, call, peer, |cx| m.backend_url(&cx));
            }),
        )
    }

    pub fn health(self) -> Self
    where
        M: Health,
    {
        self.add(
            caps::HEALTH,
            Method::Health,
            route!(|m, call, peer| {
                let Some((inst, call)) = args::<Instance>(call) else {
                    return;
                };
                with_cx!(m, inst, call, peer, |cx| m
                    .health(&cx)
                    .await
                    .map(|()| Unit {}));
            }),
        )
    }

    pub fn restarts(self) -> Self
    where
        M: Restarts,
    {
        self.add(
            caps::RESTART,
            Method::Restart,
            route!(|m, call, peer| {
                let Some((inst, call)) = args::<Instance>(call) else {
                    return;
                };
                with_cx!(m, inst, call, peer, |cx| Ok::<_, Error>(m.restart(&cx)));
            }),
        )
    }

    /// Admin: sub-resources and data actions.
    pub fn admin(self) -> Self
    where
        M: Admin,
    {
        let s = self.add(
            caps::ADMIN,
            Method::Resources,
            route!(|m, call, peer| {
                let Some((inst, call)) = args::<Instance>(call) else {
                    return;
                };
                with_cx!(m, inst, call, peer, |cx| m.resources(&cx).await);
            }),
        );
        s.add(
            caps::ADMIN,
            Method::RunAction,
            route!(|m, call, peer| {
                let Some((a, call)) = args::<RunAction>(call) else {
                    return;
                };
                with_cx!(m, a.instance, call, peer, |cx| m
                    .run_action(&cx, &a.action, &a.resource, a.input.as_deref())
                    .await);
            }),
        )
    }

    /// Console streams.
    pub fn console(self) -> Self
    where
        M: Console,
    {
        self.add(
            caps::CONSOLE,
            Method::Tail,
            route!(|m, call, peer| {
                let Some((a, call)) = args::<TailArgs>(call) else {
                    return;
                };
                match crate::server::make_cx(&*m, a.instance, call, peer) {
                    Err(b) => {
                        let (call, e) = *b;
                        drop(call.responder.err(&e))
                    }
                    Ok(cx) => {
                        let r = m
                            .tail(&cx, &a.panel, &a.selector, &Rows(&cx.call.responder))
                            .await;
                        let responder = cx.call.responder;
                        drop(match r {
                            Ok(()) => responder.end(),
                            Err(e) => responder.err(&e),
                        });
                    }
                }
            }),
        )
    }

    /// Templates: provision once, clone per instance (`doze_os::fs::clone_dir`, copy-on-write where it can).
    pub fn template(self) -> Self
    where
        M: Template,
    {
        let s = self.add(
            caps::TEMPLATE,
            Method::TemplateEnsure,
            route!(|m, call, _peer| {
                let Some((a, call)) = args::<TemplateEnsure>(call) else {
                    return;
                };
                let r = m
                    .ensure_template(&a.toolchain, std::path::Path::new(&a.template_dir))
                    .await;
                drop(match r {
                    Ok(()) => call.responder.ok(&Unit {}),
                    Err(e) => call.responder.err(&e),
                });
            }),
        );
        s.add(
            caps::TEMPLATE,
            Method::TemplateClone,
            route!(|_m, call, _peer| {
                let Some((a, call)) = args::<TemplateClone>(call) else {
                    return;
                };
                let (src, dst) = (a.template_dir.clone(), a.dest_dir.clone());
                let r = tokio::task::spawn_blocking(move || {
                    doze_os::fs::clone_dir(src.as_ref(), dst.as_ref())
                })
                .await;
                drop(match r {
                    Ok(Ok(())) => call.responder.ok(&Unit {}),
                    Ok(Err(e)) => call.responder.err(&Error::new(
                        ErrorKind::Unavailable,
                        format!("cloning the template: {e}"),
                    )),
                    Err(e) => call
                        .responder
                        .err(&Error::new(ErrorKind::Internal, e.to_string())),
                });
            }),
        )
    }
}

impl<M: Module + crate::caps::Wire> Server<M> {
    /// Client hand-off: the module takes over connections (`Wire`).
    pub fn wire(self) -> Self {
        self.add(
            caps::WIRE,
            Method::WireAccept,
            route!(|m, call, peer| {
                let Some((a, mut call)) = args::<doze_module_proto::msg::console::WireAccept>(call)
                else {
                    return;
                };
                let Some(handle) = call.handle.take() else {
                    drop(call.responder.err(&Error::new(
                        ErrorKind::Invalid,
                        "wire.accept without a connection",
                    )));
                    return;
                };
                let std_conn = std::net::TcpStream::from(handle);
                let conn = match std_conn
                    .set_nonblocking(true)
                    .and_then(|()| tokio::net::TcpStream::from_std(std_conn))
                {
                    Ok(c) => c,
                    Err(e) => {
                        drop(call.responder.err(&Error::new(
                            ErrorKind::Internal,
                            format!("the handed-over connection: {e}"),
                        )));
                        return;
                    }
                };
                let host = crate::caps::WireHost {
                    peer,
                    for_call: call.id(),
                    instance: a.instance.clone(),
                };
                let r = m.accept(conn, &a, &host).await;
                drop(match r {
                    Ok(()) => call.responder.ok(&Unit {}),
                    Err(e) => call.responder.err(&e),
                });
            }),
        )
    }
}
