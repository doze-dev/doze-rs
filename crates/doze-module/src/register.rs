//! Registering capabilities on the server. Each method needs its trait, sets its capability bit, and routes its calls.

use crate::caps::{Admin, Attributes, BackendUrl, Converge, Health, Objects, Prune, Restarts};
use crate::module::Module;
use crate::route;
use crate::route::args;
use crate::server::{Route, Server};
use crate::with_cx;
use doze_module_proto::method::Method;
use doze_module_proto::msg::calls::{PruneArgs, RunAction, Unit};
use doze_module_proto::msg::error::Error;
use doze_module_proto::msg::hello::caps;
use doze_module_proto::msg::instance::Instance;
use doze_module_proto::msg::run::WithToolchain;
use doze_module_proto::peer::Incoming;
use std::sync::Arc;

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
}
