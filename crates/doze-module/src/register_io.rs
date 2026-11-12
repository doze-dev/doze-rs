//! Registering the capabilities that stream or hand over: the console, templates, client connections.

use crate::caps::{Console, Rows, Template};
use crate::module::Module;
use crate::route;
use crate::route::args;
use crate::server::{Route, Server};
use doze_module_proto::method::Method;
use doze_module_proto::msg::calls::{TemplateClone, TemplateEnsure, Unit};
use doze_module_proto::msg::console::TailArgs;
use doze_module_proto::msg::error::{Error, ErrorKind};
use doze_module_proto::msg::hello::caps;
use doze_module_proto::peer::Incoming;
use std::sync::Arc;

impl<M: Module> Server<M> {
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
