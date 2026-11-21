//! Step Functions: state machines (their versions, aliases and tags are tabs of a machine's page; its executions are pages of
//! their own) and the activities workers poll for. A machine is made in the console's Workflow Studio (the page writes the
//! Amazon States Language and the call is `CreateStateMachine`).

use super::Descriptor;

mod activities;
mod machines;

pub(super) static STEPFUNCTIONS: Descriptor = Descriptor {
    service: "stepfunctions",
    target: "states",
    kinds: &[machines::MACHINES, activities::ACTIVITIES],
};
