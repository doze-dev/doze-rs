//! The daemon's operations as typed calls on a connection (Go's `control.Daemon` and `Client` helpers). Each unary
//! call waits at most the connection's timeout.

use super::connection::Connection;
use super::stream::Stream;
use crate::{
    ActionView, Error, InstanceView, LogFrame, LogQuery, Op, Request, ResourceView, Response,
    Snapshot, Verb,
};

impl Connection {
    /// The stack right now, with each running instance's meters (`status`).
    pub fn snapshot(&self) -> Result<Snapshot, Error> {
        self.call(Request::op(Op::Status)).map(Snapshot::from)
    }

    /// Runs a verb on its target and waits for it.
    pub fn run_verb(&self, verb: Verb, target: &str) -> Result<(), Error> {
        self.call(Request::verb(verb, target)).map(drop)
    }

    /// Reads lines an instance printed; it need not be running (`logs`).
    pub fn logs(&self, q: &LogQuery) -> Result<Vec<LogFrame>, Error> {
        self.call(Request::logs(q)).map(|r| r.log)
    }

    /// A builtin instance's sub-resources and the data actions it offers (`resources`).
    pub fn resources(&self, instance: &str) -> Result<(Vec<ResourceView>, Vec<ActionView>), Error> {
        self.call(Request::on(Op::Resources, instance))
            .map(|r| (r.resources, r.actions))
    }

    /// Runs a module's data action on a resource of it and returns what it said (`admin`).
    pub fn admin(
        &self,
        instance: &str,
        action: &str,
        resource: &str,
        input: &str,
    ) -> Result<String, Error> {
        self.call(Request::admin(instance, action, resource, input))
            .map(|r| r.result)
    }

    /// Wires a rendered HCL block into the running stack and returns the new instance (`add`).
    pub fn add(&self, block: &str) -> Result<InstanceView, Error> {
        let r: Response = self.call(Request::add(block))?;
        r.instances
            .into_iter()
            .next()
            .ok_or_else(|| Error::NoResponse("add".to_owned()))
    }

    /// Tears an instance down, optionally wiping its data (`remove`).
    pub fn remove(&self, name: &str, wipe: bool) -> Result<(), Error> {
        self.call(Request::remove(name, wipe)).map(drop)
    }

    /// Follows instance-state transitions (`events`): each arrives as an instances frame.
    pub fn events(&self) -> Result<Stream, Error> {
        self.stream(Request::op(Op::Events))
    }

    /// Follows the logs of `names` (empty = every running backend): `logs` with `follow`.
    pub fn follow_logs(&self, names: &[String]) -> Result<Stream, Error> {
        self.stream(Request::follow_logs(names))
    }
}
