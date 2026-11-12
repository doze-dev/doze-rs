//! Every call of protocol 2, with who makes it, its shape, the capability it needs, the call that scopes it, and the
//! minor version it appeared in. The peer enforces this table; nothing else decides what may be called.

use crate::msg::hello::caps;

/// Who calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    HostToModule,
    ModuleToHost,
}

/// One answer, or a stream of items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Unary,
    Stream,
}

macro_rules! methods {
    ($($name:ident = $id:literal, $dir:ident, $shape:ident, $cap:expr, $scope:expr, $since:literal;)*) => {
        /// A call of protocol 2.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u16)]
        pub enum Method { $($name = $id,)* }

        impl Method {
            /// Every method, for tables and tests.
            pub const ALL: &'static [Method] = &[$(Method::$name,)*];

            /// The method with this wire id, if this build knows it.
            pub fn from_u16(id: u16) -> Option<Method> {
                match id { $($id => Some(Method::$name),)* _ => None }
            }

            pub fn direction(self) -> Direction {
                match self { $(Method::$name => Direction::$dir,)* }
            }

            pub fn shape(self) -> Shape {
                match self { $(Method::$name => Shape::$shape,)* }
            }

            /// The capability bit the module must have declared (0: always available).
            pub fn capability(self) -> u64 {
                match self { $(Method::$name => $cap,)* }
            }

            /// For a callback: the call that must be in flight (and named in `for_call`) for it to be allowed.
            pub fn scope(self) -> Option<Method> {
                match self { $(Method::$name => $scope,)* }
            }

            /// The protocol minor that introduced it.
            pub fn since(self) -> u16 {
                match self { $(Method::$name => $since,)* }
            }
        }
    };
}

methods! {
    Hello = 1, ModuleToHost, Unary, 0, None, 0;
    Decode = 10, HostToModule, Unary, 0, None, 0;
    Network = 11, HostToModule, Unary, 0, None, 0;
    Connection = 12, HostToModule, Unary, 0, None, 0;
    Attributes = 13, HostToModule, Unary, caps::ATTRIBUTES, None, 0;
    BackendUrl = 14, HostToModule, Unary, caps::BACKEND_URL, None, 0;
    Resolve = 20, HostToModule, Unary, 0, None, 0;
    Provision = 21, HostToModule, Unary, 0, None, 0;
    Plan = 22, HostToModule, Unary, 0, None, 0;
    TemplateEnsure = 23, HostToModule, Unary, caps::TEMPLATE, None, 0;
    TemplateClone = 24, HostToModule, Unary, caps::TEMPLATE, None, 0;
    Converge = 30, HostToModule, Unary, caps::CONVERGE, None, 0;
    Objects = 31, HostToModule, Unary, caps::OBJECTS, None, 0;
    Prune = 32, HostToModule, Unary, caps::PRUNE, None, 0;
    Restart = 40, HostToModule, Unary, caps::RESTART, None, 0;
    Health = 41, HostToModule, Unary, caps::HEALTH, None, 0;
    Resources = 50, HostToModule, Unary, caps::ADMIN, None, 0;
    RunAction = 51, HostToModule, Unary, caps::ADMIN, None, 0;
    Tail = 52, HostToModule, Stream, caps::CONSOLE, None, 0;
    WireAccept = 60, HostToModule, Unary, caps::WIRE, None, 0;
    Shutdown = 70, HostToModule, Unary, 0, None, 0;
    FetchResolveMajor = 100, ModuleToHost, Unary, 0, Some(Method::Resolve), 0;
    FetchEnsure = 101, ModuleToHost, Unary, 0, Some(Method::Resolve), 0;
    WireBoot = 110, ModuleToHost, Unary, caps::WIRE, Some(Method::WireAccept), 0;
    WireAcquire = 111, ModuleToHost, Unary, caps::WIRE, Some(Method::WireAccept), 0;
    WireRelease = 112, ModuleToHost, Unary, caps::WIRE, Some(Method::WireAccept), 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_callbacks_are_scoped_to_host_calls() {
        for m in Method::ALL {
            assert_eq!(Method::from_u16(*m as u16), Some(*m));
            if let Some(scope) = m.scope() {
                assert_eq!(m.direction(), Direction::ModuleToHost, "{m:?}");
                assert_eq!(
                    scope.direction(),
                    Direction::HostToModule,
                    "{m:?} scoped to {scope:?}"
                );
            }
        }
        assert_eq!(Method::from_u16(9999), None);
    }
}
