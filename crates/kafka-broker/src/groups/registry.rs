//! The groups that exist: found by id, started by their first JoinGroup, retired by DeleteGroups.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, PoisonError};
use std::time::Duration;

use kafka_meta::{GroupLife, MetaStore};
use kafka_wire::ResponseError;
use kafka_wire::ids::{GroupId, InstanceId, MemberId};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::Instant;

use super::actor::{self, Command, Records, Req};
use super::machine::{
    Answer, Description, GroupMachine, IdSource, JoinAnswer, LeaveResult, SyncAnswer,
    commit_to_no_group, refusal,
};

/// How many requests may wait for a group's task before the next sender waits.
const QUEUE: usize = 64;
/// How long a member id handed to a client that has not yet come back with it is good for.
const ISSUED_FOR: Duration = Duration::from_secs(300);
/// The most ids handed out and not yet come back with. A client that only ever asks for an id costs an entry here, not a
/// group.
const MOST_ISSUED: usize = 100_000;

/// What became of a request to delete a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Deleted {
    /// There was no such group in memory.
    NoSuchGroup,
    /// It has members.
    NotEmpty,
    /// It is gone.
    Gone,
}

/// The classic consumer groups of the broker.
#[derive(Debug)]
pub(crate) struct Groups {
    initial_delay_ms: u64,
    meta: Arc<MetaStore>,
    /// Held while a group is looked up, made or retired; never while a request waits for its answer, except a
    /// deletion, so that a group is never made again while the old one is going.
    open: Mutex<HashMap<GroupId, mpsc::Sender<Command>>>,
    /// Ids handed to clients (JoinGroup v4 and later tells a client without one to come back with the one it is given),
    /// kept here and not in the group so that a client that never comes back leaves no group behind.
    issued: StdMutex<HashMap<(GroupId, MemberId), Instant>>,
}

impl Groups {
    pub(crate) fn new(initial_delay_ms: u64, meta: Arc<MetaStore>) -> Self {
        Self {
            initial_delay_ms,
            meta,
            open: Mutex::new(HashMap::new()),
            issued: StdMutex::new(HashMap::new()),
        }
    }

    /// Puts `req` to the group and waits for the answer. `None`: the request was replaced by a later one (a
    /// JoinGroup sent again) or the group has gone, and will not be answered.
    ///
    /// A group is made by a JoinGroup; any other request to a group that does not exist is answered as an empty
    /// group would.
    pub(crate) async fn ask(&self, group: &GroupId, mut req: Req) -> Option<Answer> {
        let mut spends: Option<MemberId> = None;
        if let Req::Join(join) = &mut req {
            if let Some(error) = refusal(join) {
                return Some(Answer::Join(JoinAnswer::refused(error, None)));
            }
            if join.id_first && join.member_id.is_none() && join.instance.is_none() {
                return Some(self.issue(group));
            }
            if let Some(id) = &join.member_id
                && self.is_issued(group, id)
            {
                join.issued = true;
                spends = Some(id.clone());
            }
        }
        let answer = self.put(group, req).await;
        // The id is used up by a join that is taken, not by one that is refused.
        if let (Some(id), Some(Answer::Join(joined))) = (&spends, &answer)
            && joined.error.is_none()
        {
            self.spend(group, id);
        }
        answer
    }

    fn issued(&self) -> std::sync::MutexGuard<'_, HashMap<(GroupId, MemberId), Instant>> {
        self.issued.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hands a client without an id one to come back with (MEMBER_ID_REQUIRED).
    fn issue(&self, group: &GroupId) -> Answer {
        let now = Instant::now();
        let mut issued = self.issued();
        issued.retain(|_, at| now.duration_since(*at) < ISSUED_FOR);
        if issued.len() >= MOST_ISSUED {
            return Answer::Join(JoinAnswer::refused(
                ResponseError::CoordinatorNotAvailable,
                None,
            ));
        }
        let id = new_member_id(None);
        issued.insert((group.clone(), id.clone()), now);
        Answer::Join(JoinAnswer::refused(
            ResponseError::MemberIdRequired,
            Some(id),
        ))
    }

    fn is_issued(&self, group: &GroupId, id: &MemberId) -> bool {
        self.issued()
            .get(&(group.clone(), id.clone()))
            .is_some_and(|at| Instant::now().duration_since(*at) < ISSUED_FOR)
    }

    fn spend(&self, group: &GroupId, id: &MemberId) {
        self.issued().remove(&(group.clone(), id.clone()));
    }

    /// Puts the request to the group's task.
    async fn put(&self, group: &GroupId, mut req: Req) -> Option<Answer> {
        // A group retired between the lookup and the send is found gone, and a join makes it again.
        for _ in 0..2 {
            let sender = {
                let mut open = self.open.lock().await;
                match open.get(group) {
                    Some(sender) => sender.clone(),
                    None if matches!(req, Req::Join(_)) => {
                        let sender = self.start(group);
                        open.insert(group.clone(), sender.clone());
                        sender
                    }
                    None => return unknown(&req),
                }
            };
            let (reply, answer) = oneshot::channel();
            match sender.send(Command { req, reply }).await {
                Ok(()) => return answer.await.ok(),
                Err(gone) => {
                    req = gone.0.req;
                    let mut open = self.open.lock().await;
                    if open.get(group).is_some_and(|s| s.same_channel(&sender)) {
                        open.remove(group);
                    }
                }
            }
        }
        None
    }

    /// The group as DescribeGroups shows it, if it is in memory.
    pub(crate) async fn describe(&self, group: &GroupId) -> Option<Description> {
        match self.ask(group, Req::Describe).await {
            Some(Answer::Describe(d)) => Some(d),
            _ => None,
        }
    }

    /// Every group in memory with its description, in no particular order.
    pub(crate) async fn list(&self) -> Vec<(GroupId, Description)> {
        let all: Vec<(GroupId, mpsc::Sender<Command>)> = {
            let open = self.open.lock().await;
            open.iter().map(|(g, s)| (g.clone(), s.clone())).collect()
        };
        let mut out = Vec::with_capacity(all.len());
        for (group, sender) in all {
            let (reply, answer) = oneshot::channel();
            let command = Command {
                req: Req::Describe,
                reply,
            };
            if sender.send(command).await.is_ok()
                && let Ok(Answer::Describe(d)) = answer.await
            {
                out.push((group, d));
            }
        }
        out
    }

    /// Deletes a group that has no members.
    pub(crate) async fn delete(&self, group: &GroupId) -> Deleted {
        // Held for the whole of it: a join that arrives meanwhile waits, and then makes a new group.
        let mut open = self.open.lock().await;
        let Some(sender) = open.get(group).cloned() else {
            return Deleted::NoSuchGroup;
        };
        let (reply, answer) = oneshot::channel();
        let command = Command {
            req: Req::Delete,
            reply,
        };
        if sender.send(command).await.is_err() {
            open.remove(group);
            return Deleted::NoSuchGroup;
        }
        match answer.await {
            Ok(Answer::Delete(None)) => {
                open.remove(group);
                Deleted::Gone
            }
            Ok(Answer::Delete(Some(_))) => Deleted::NotEmpty,
            _ => {
                open.remove(group);
                Deleted::NoSuchGroup
            }
        }
    }

    fn start(&self, group: &GroupId) -> mpsc::Sender<Command> {
        let (sender, commands) = mpsc::channel(QUEUE);
        let machine = GroupMachine::new(self.initial_delay_ms, member_ids());
        let records = StoreRecords {
            meta: Arc::clone(&self.meta),
            group: group.clone(),
        };
        tokio::spawn(actor::run(machine, commands, records));
        sender
    }
}

/// A group's records, in the metadata database.
struct StoreRecords {
    meta: Arc<MetaStore>,
    group: GroupId,
}

impl StoreRecords {
    fn write(&self, life: &GroupLife) -> bool {
        match crate::blocking::run(|| self.meta.set_group_life(&self.group, life)) {
            Ok(()) => true,
            Err(e) => {
                tracing::error!(group = %self.group, error = %e, "writing what is known of a group's members failed");
                false
            }
        }
    }
}

impl Records for StoreRecords {
    fn live(&self, session_ms: u32, protocol_type: &str) -> bool {
        self.write(&GroupLife {
            live: true,
            session_ms,
            empty_since_ms: None,
            protocol_type: protocol_type.to_owned(),
        })
    }

    fn empty(&self, protocol_type: &str, now_ms: i64) -> bool {
        self.write(&GroupLife {
            live: false,
            session_ms: 0,
            empty_since_ms: Some(now_ms),
            protocol_type: protocol_type.to_owned(),
        })
    }
}

/// What a group that does not exist says to anything but a join: it has no such member. (A request to describe or
/// delete one is not put at all.)
fn unknown(req: &Req) -> Option<Answer> {
    Some(match req {
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
        Req::Describe | Req::Delete => return None,
    })
}

/// Member ids: `doze-` and 32 hex digits, behind the instance id for a static member.
fn member_ids() -> IdSource {
    Box::new(new_member_id)
}

fn new_member_id(instance: Option<&InstanceId>) -> MemberId {
    let mut bytes = [0_u8; 16];
    // Without randomness ids repeat, which a group notices as a member that is not itself; there is nothing better
    // to do than carry on.
    let _ = getrandom::fill(&mut bytes);
    MemberId::issued(bytes, instance)
}
