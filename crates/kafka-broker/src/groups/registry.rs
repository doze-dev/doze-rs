//! The groups that exist: found by id, started by their first JoinGroup.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use kafka_wire::ResponseError;
use kafka_wire::ids::{GroupId, InstanceId, MemberId};
use tokio::sync::{mpsc, oneshot};

use super::actor::{self, Command, Req};
use super::machine::{
    Answer, GroupMachine, IdSource, JoinAnswer, LeaveResult, SyncAnswer, commit_to_no_group,
};

/// How many requests may wait for a group's task before the next sender waits.
const QUEUE: usize = 64;

/// The classic consumer groups of the broker.
#[derive(Debug)]
pub(crate) struct Groups {
    initial_delay_ms: u64,
    open: Mutex<HashMap<GroupId, mpsc::Sender<Command>>>,
}

impl Groups {
    pub(crate) fn new(initial_delay_ms: u64) -> Self {
        Self {
            initial_delay_ms,
            open: Mutex::new(HashMap::new()),
        }
    }

    /// Puts `req` to the group and waits for the answer. `None`: the request was replaced by a later one (a
    /// JoinGroup sent again) or the group has gone, and will not be answered.
    ///
    /// A group is made by a JoinGroup; any other request to a group that does not exist is answered as an empty
    /// group would.
    pub(crate) async fn ask(&self, group: &GroupId, req: Req) -> Option<Answer> {
        let sender = {
            let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
            match open.get(group) {
                Some(sender) => sender.clone(),
                None if matches!(req, Req::Join(_)) => {
                    let sender = self.start();
                    open.insert(group.clone(), sender.clone());
                    sender
                }
                None => return Some(unknown(&req)),
            }
        };
        let (reply, answer) = oneshot::channel();
        sender.send(Command { req, reply }).await.ok()?;
        answer.await.ok()
    }

    fn start(&self) -> mpsc::Sender<Command> {
        let (sender, commands) = mpsc::channel(QUEUE);
        let machine = GroupMachine::new(self.initial_delay_ms, member_ids());
        tokio::spawn(actor::run(machine, commands));
        sender
    }
}

/// What a group that does not exist says to anything but a join: it has no such member.
fn unknown(req: &Req) -> Answer {
    match req {
        Req::Join(_) => Answer::Join(JoinAnswer::refused(ResponseError::UnknownMemberId, None)),
        Req::Sync(_) => Answer::Sync(SyncAnswer::refused(ResponseError::UnknownMemberId)),
        Req::Heartbeat(_) => Answer::Heartbeat(Some(ResponseError::UnknownMemberId)),
        Req::Commit(c) => Answer::Commit(commit_to_no_group(c.generation)),
        Req::Leave(targets) => Answer::Leave(
            targets
                .iter()
                .map(|t| LeaveResult {
                    member_id: t.member_id.clone(),
                    instance: t.instance.clone(),
                    error: Some(ResponseError::UnknownMemberId),
                })
                .collect(),
        ),
    }
}

/// Member ids: `doze-` and 32 hex digits, behind the instance id for a static member.
fn member_ids() -> IdSource {
    Box::new(|instance: Option<&InstanceId>| {
        let mut bytes = [0_u8; 16];
        // Without randomness ids repeat, which a group notices as a member that is not itself; there is nothing
        // better to do than carry on.
        let _ = getrandom::fill(&mut bytes);
        MemberId::issued(bytes, instance)
    })
}
