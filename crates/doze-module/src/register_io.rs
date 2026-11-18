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
                let r = tokio::task::spawn_blocking(move || clone_into(src.as_ref(), dst.as_ref()))
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
                // A client gone without a word must not hold its instance awake for ever.
                let _ = doze_os::net::keepalive(&std_conn);
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

/// The template cloned to `dst`, its parent made first (private, as Go's `Clone` made it). The clone is made beside
/// `dst` and renamed into place, so one that failed half way (a full disk, a killed module) leaves no half data
/// directory: the next attempt clears the leftover and clones again. An empty `dst` left by an earlier attempt is
/// removed; a `dst` holding anything is refused, never overwritten. Not made durable: a power cut during the plain-copy
/// fallback can leave a renamed `dst` with files short (copy-on-write clones are whole or absent); it is the
/// host's to not start two clones of one `dst` at once (it boots an instance once).
fn clone_into(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        doze_os::fs::private_dir(parent)?;
    }
    match std::fs::read_dir(dst).map(|mut d| d.next().is_none()) {
        Ok(true) => std::fs::remove_dir(dst)?,
        Ok(false) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("{} already holds data", dst.display()),
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut name = std::ffi::OsString::from(".");
    name.push(dst.file_name().unwrap_or_default());
    name.push(".cloning");
    let part = dst.with_file_name(name);
    if std::fs::symlink_metadata(&part).is_ok() {
        std::fs::remove_dir_all(&part)?;
    }
    doze_os::fs::clone_dir(src, &part)?;
    std::fs::rename(&part, dst)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::clone_into;

    #[test]
    fn a_clone_that_failed_half_way_is_cleared_and_made_again() {
        let root = std::env::temp_dir().join(format!("doze-module-clone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (src, dst) = (root.join("template"), root.join("data/pg"));
        std::fs::create_dir_all(src.join("base")).unwrap();
        std::fs::write(src.join("base/1"), "one").unwrap();
        // What a killed earlier attempt leaves: part of a clone beside the data directory, whose folder that attempt had
        // made private first (on Windows a protected DACL; a folder made before it would be left with no access at all).
        doze_os::fs::private_dir(&root.join("data")).unwrap();
        std::fs::create_dir_all(root.join("data/.pg.cloning/base")).unwrap();
        std::fs::write(root.join("data/.pg.cloning/half"), "x").unwrap();
        clone_into(&src, &dst).unwrap();
        assert_eq!(std::fs::read_to_string(dst.join("base/1")).unwrap(), "one");
        assert!(!dst.join("half").exists());
        assert!(!root.join("data/.pg.cloning").exists());
        // A data directory holding anything is never overwritten.
        let e = clone_into(&src, &dst).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
