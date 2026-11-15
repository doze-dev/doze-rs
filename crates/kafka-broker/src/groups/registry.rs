//! The groups that exist: found by id, started by their first JoinGroup, retired by DeleteGroups.
//!
//! The table of groups is behind a plain mutex that is never held across an `.await`: it is looked in, changed, and let
//! go. A group being deleted keeps its place in the table, marked as going, until what is stored of it is forgotten; a
//! request for it meanwhile waits for that one deletion, and nothing else does.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use kafka_meta::MetaStore;
use kafka_wire::ResponseError;
use kafka_wire::ids::GroupId;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

mod issued;
mod records;
mod requests;

use super::actor::{self, Command, Reply, Req};
use super::consumer::TopicSource;
use super::consumer::assign::Assignor;
use super::events::{Events, Ledger, Recorded};
use super::group::{Group, Settings, asks_for_an_id};
use super::machine::{Answer, Description, Force, Forced, JoinAnswer, JoinRequest, refusal};
pub(crate) use issued::member_ids;
use issued::{Issued, MOST_ISSUED};
use records::{MetaTopics, StoreRecords};
use requests::{can_begin, unknown};

/// How many requests may wait for a group's task before the next sender waits.
const QUEUE: usize = 64;
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

/// A group's place in the table.
#[derive(Debug)]
enum Slot {
    /// Its task, taking requests.
    Live(mpsc::Sender<Command>),
    /// Being deleted: the deletion holds the sending side, and lets go of it when it is done (or given up), which wakes
    /// whoever waits here. A closed channel cannot be missed the way a notification sent before the wait began can.
    Deleting(watch::Receiver<()>),
}

/// The classic consumer groups of the broker.
#[derive(Debug)]
pub(crate) struct Groups {
    settings: Settings,
    meta: Arc<MetaStore>,
    /// The topics, as every group is handed them.
    topics: Arc<MetaTopics>,
    /// The most ids that may be outstanding at once.
    most_issued: usize,
    /// Every group in memory. Never held across an `.await`.
    open: Mutex<HashMap<GroupId, Slot>>,
    /// Ids handed to clients (JoinGroup v4 and later tells a client without one to come back with the one it is given),
    /// kept here and not in the group so that a client that never comes back leaves no group behind.
    issued: Mutex<Issued>,
    /// Which groups hold a ring of what they have said of themselves (feature `inspect` reads them).
    events: Arc<Ledger>,
}

/// Where a request is to go.
enum Found {
    Live(mpsc::Sender<Command>),
    /// The group is being deleted: wait, then look again.
    Going(watch::Receiver<()>),
    /// There is no such group, and the request cannot make one.
    Absent,
    /// There is no such group, and the request makes it.
    Begins,
}

impl Groups {
    pub(crate) fn new(settings: Settings, meta: Arc<MetaStore>) -> Self {
        Self {
            settings,
            topics: Arc::new(MetaTopics::new(Arc::clone(&meta))),
            meta,
            most_issued: MOST_ISSUED,
            open: Mutex::new(HashMap::new()),
            issued: Mutex::new(Issued {
                ids: HashMap::new(),
                per_group: HashMap::new(),
                purged: Instant::now(),
            }),
            events: Arc::new(Ledger::default()),
        }
    }

    fn table(&self) -> MutexGuard<'_, HashMap<GroupId, Slot>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The task of a group in memory that is not being deleted.
    fn live(&self, group: &GroupId) -> Option<mpsc::Sender<Command>> {
        match self.table().get(group) {
            Some(Slot::Live(sender)) => Some(sender.clone()),
            _ => None,
        }
    }

    /// Puts `req` to the group and waits for the answer. `None`: the request was replaced by a later one (a
    /// JoinGroup sent again) or the group has gone, and will not be answered.
    ///
    /// A group is made by a JoinGroup; any other request to a group that does not exist is answered as an empty
    /// group would.
    pub(crate) async fn ask(&self, group: &GroupId, req: Req) -> Option<Answer> {
        let Req::Join(mut join) = req else {
            return self.put(group, req).await;
        };
        if let Some(error) = refusal(&join) {
            return Some(Answer::Join(JoinAnswer::refused(error, None)));
        }
        if asks_for_an_id(&join) {
            // A group that is there may refuse the client before it is given an id; one that is not there is not made.
            let session_ms = join.session_ms;
            return Some(match self.vet(group, join).await {
                Some(refused) => refused,
                None => self.issue(group, session_ms),
            });
        }
        // The id is used up by a join that is taken, not by one that is refused: kept to spend after the join is put.
        let spends = join
            .member_id
            .as_ref()
            .filter(|id| self.is_issued(group, id))
            .cloned();
        if spends.is_some() {
            join.issued = true;
        }
        let answer = self.put(group, Req::Join(join)).await;
        if let (Some(id), Some(Answer::Join(joined))) = (&spends, &answer)
            && joined.error.is_none()
        {
            self.spend(group, id);
        }
        answer
    }

    /// What a group that exists says to a client asking for an id, if it refuses it. `None`: it may be given one. The
    /// join is the group's to look at and is not put anywhere after.
    async fn vet(&self, group: &GroupId, join: JoinRequest) -> Option<Answer> {
        // A group being deleted is as good as not there: an id makes no group.
        let sender = self.live(group)?;
        let (reply, answer) = oneshot::channel();
        let command = Command {
            req: Req::Join(join),
            reply,
        };
        sender.send(command).await.ok()?;
        match answer.await.ok()? {
            Reply::Answer(Answer::Join(j)) if j.error == Some(ResponseError::MemberIdRequired) => {
                None
            }
            Reply::Answer(other) => Some(other),
            // Deleted before it looked: there is no group to refuse it.
            Reply::Retired(_) => None,
        }
    }

    /// Where a request for `group` goes now.
    fn find(&self, group: &GroupId, req: &Req) -> Found {
        match self.table().get(group) {
            Some(Slot::Live(sender)) => Found::Live(sender.clone()),
            Some(Slot::Deleting(done)) => Found::Going(done.clone()),
            None if can_begin(req) => Found::Begins,
            None => Found::Absent,
        }
    }

    /// The task of `group`, made if the request may begin it. `None`: there is no group, and the request makes none.
    async fn sender_for(&self, group: &GroupId, req: &Req) -> Option<mpsc::Sender<Command>> {
        loop {
            match self.find(group, req) {
                Found::Live(sender) => return Some(sender),
                // Woken when the deletion lets go; then the table says what there is.
                Found::Going(mut done) => {
                    let _ = done.changed().await;
                }
                Found::Absent => return None,
                Found::Begins => {
                    // A consumer group is made from its record, read before the table is locked again; a join that made
                    // the group meanwhile wins, and this machine is let go.
                    let machine = self.machine_for(group, req);
                    let mut table = self.table();
                    match table.get(group) {
                        Some(Slot::Live(sender)) => return Some(sender.clone()),
                        Some(Slot::Deleting(_)) => {}
                        None => {
                            let sender = self.spawn(group, machine);
                            table.insert(group.clone(), Slot::Live(sender.clone()));
                            return Some(sender);
                        }
                    }
                }
            }
        }
    }

    /// Puts the request to the group's task.
    async fn put(&self, group: &GroupId, mut req: Req) -> Option<Answer> {
        // A group retired between the lookup and the send is found gone, and a join makes it again.
        for _ in 0..3 {
            let Some(sender) = self.sender_for(group, &req).await else {
                return unknown(&req);
            };
            let (reply, answer) = oneshot::channel();
            match sender.send(Command { req, reply }).await {
                Ok(()) => match answer.await {
                    Ok(Reply::Answer(answered)) => return Some(answered),
                    // Queued behind the group's deletion and handed back: to a group made afresh by a join, or to none,
                    // which answers it as a group with no members would.
                    Ok(Reply::Retired(back)) => req = back,
                    // Dropped without an answer: replaced by a later join.
                    Err(_) => return None,
                },
                Err(gone) => req = gone.0.req,
            }
            let mut table = self.table();
            if let Some(Slot::Live(s)) = table.get(group)
                && s.same_channel(&sender)
            {
                table.remove(group);
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

    /// Forces a step on the group (the console's Lab).
    #[cfg_attr(not(feature = "inspect"), allow(dead_code))]
    pub(crate) async fn force(&self, group: &GroupId, f: Force) -> Option<Forced> {
        match self.ask(group, Req::Force(f)).await {
            Some(Answer::Forced(done)) => Some(done),
            _ => None,
        }
    }

    /// What the group has said of itself, oldest first, if it is in memory. Asking changes nothing.
    #[cfg_attr(not(feature = "inspect"), allow(dead_code))]
    pub(crate) async fn events(&self, group: &GroupId) -> Option<Vec<Recorded>> {
        match self.ask(group, Req::Events).await {
            Some(Answer::Events(events)) => Some(events),
            _ => None,
        }
    }

    /// Every group in memory with its description, in no particular order.
    pub(crate) async fn list(&self) -> Vec<(GroupId, Description)> {
        let all: Vec<(GroupId, mpsc::Sender<Command>)> = self
            .table()
            .iter()
            .filter_map(|(g, slot)| match slot {
                Slot::Live(s) => Some((g.clone(), s.clone())),
                Slot::Deleting(_) => None,
            })
            .collect();
        let mut out = Vec::with_capacity(all.len());
        for (group, sender) in all {
            let (reply, answer) = oneshot::channel();
            let command = Command {
                req: Req::Describe,
                reply,
            };
            if sender.send(command).await.is_ok()
                && let Ok(Reply::Answer(Answer::Describe(d))) = answer.await
            {
                out.push((group, d));
            }
        }
        out
    }

    /// Tells every group in memory that a topic was made, grown or deleted. Not waited for.
    pub(crate) async fn topics_changed(&self) {
        let senders: Vec<mpsc::Sender<Command>> = self
            .table()
            .values()
            .filter_map(|slot| match slot {
                Slot::Live(s) => Some(s.clone()),
                Slot::Deleting(_) => None,
            })
            .collect();
        for sender in senders {
            let (reply, _) = oneshot::channel();
            let _ = sender
                .send(Command {
                    req: Req::TopicsChanged,
                    reply,
                })
                .await;
        }
    }

    /// Deletes a group that has no members, and runs `cleanup` (forgetting what is stored of it) before any join can make
    /// the group again: a group made meanwhile would have its record wiped by it. Not run if the group has members.
    ///
    /// Only this group waits meanwhile: its place in the table says it is going until `cleanup` has run.
    pub(crate) async fn delete_with<R>(
        &self,
        group: &GroupId,
        cleanup: impl FnOnce() -> R,
    ) -> (Deleted, Option<R>) {
        let mut going = self.begin_deleting(group).await;
        let outcome = match &going.was {
            None => Deleted::NoSuchGroup,
            Some(sender) => {
                let (reply, answer) = oneshot::channel();
                let command = Command {
                    req: Req::Delete,
                    reply,
                };
                // A group whose queue is full is busy, so it has members: this is not the place to wait.
                match sender.try_send(command) {
                    Err(mpsc::error::TrySendError::Full(_)) => return (Deleted::NotEmpty, None),
                    Err(mpsc::error::TrySendError::Closed(_)) => Deleted::NoSuchGroup,
                    Ok(()) => match answer.await {
                        Ok(Reply::Answer(Answer::Delete(None))) => Deleted::Gone,
                        Ok(Reply::Answer(Answer::Delete(Some(_)))) => {
                            return (Deleted::NotEmpty, None);
                        }
                        _ => Deleted::NoSuchGroup,
                    },
                }
            }
        };
        // Gone, or never there: when the deletion is let go, its place is empty.
        going.was = None;
        let cleaned = cleanup();
        drop(going);
        (outcome, Some(cleaned))
    }

    /// Marks `group` as being deleted, once any other deletion of it has finished.
    async fn begin_deleting<'a>(&'a self, group: &'a GroupId) -> Deleting<'a> {
        loop {
            let mut done = {
                let mut table = self.table();
                match table.get(group) {
                    Some(Slot::Deleting(done)) => done.clone(),
                    _ => {
                        let (lets_go, done) = watch::channel(());
                        let was = match table.insert(group.clone(), Slot::Deleting(done)) {
                            Some(Slot::Live(sender)) => Some(sender),
                            _ => None,
                        };
                        return Deleting {
                            groups: self,
                            group,
                            was,
                            _lets_go: lets_go,
                        };
                    }
                }
            };
            let _ = done.changed().await;
        }
    }

    /// The machine a request that begins `group` starts it with: a group of the new protocol is as it was when the broker
    /// last stopped, if it was empty then.
    fn machine_for(&self, group: &GroupId, first: &Req) -> Group {
        if matches!(first, Req::ShareHeartbeat(_)) {
            return self.share_machine_for(group);
        }
        let life = crate::blocking::run(|| self.meta.group_life(group))
            .ok()
            .flatten();
        // A share group stays one, whatever asks for it.
        if let Some(share) = life.as_ref().and_then(|l| l.share.as_ref()) {
            return Group::share(self.settings, Some(share));
        }
        if !matches!(first, Req::ConsumerHeartbeat(_)) {
            return Group::classic(self.settings);
        }
        match life.and_then(|l| l.consumer) {
            Some(c) => Group::restored(
                self.settings,
                c.epoch,
                c.assignment_epoch,
                Assignor::parse(&c.assignor).unwrap_or_default(),
            ),
            None => Group::consumer(self.settings),
        }
    }

    /// The machine a share heartbeat finds for a group not in memory: the share group it was when the broker last stopped,
    /// or the consumer or classic group the id was used as (which refuses it: Kafka converts neither to a share group),
    /// or a new share group.
    fn share_machine_for(&self, group: &GroupId) -> Group {
        let (life, has_offsets) = crate::blocking::run(|| {
            let life = self.meta.group_life(group).ok().flatten();
            let offsets = self
                .meta
                .group_offsets(group)
                .is_ok_and(|o| !o.is_empty());
            (life, offsets)
        });
        match life {
            Some(l) if l.share.is_some() => Group::share(self.settings, l.share.as_ref()),
            Some(l) if l.consumer.is_some() => Group::consumer(self.settings),
            Some(_) => Group::classic(self.settings),
            None if has_offsets => Group::classic(self.settings),
            None => Group::share(self.settings, None),
        }
    }

    fn spawn(&self, group: &GroupId, machine: Group) -> mpsc::Sender<Command> {
        let (sender, commands) = mpsc::channel(QUEUE);
        let records = StoreRecords {
            meta: Arc::clone(&self.meta),
            group: group.clone(),
        };
        tokio::spawn(actor::run(
            machine,
            commands,
            records,
            Arc::clone(&self.topics) as Arc<dyn TopicSource>,
            Events::new(Arc::clone(&self.events)),
        ));
        sender
    }
}

/// A deletion under way. Dropped, it gives the group's place back: to its task if the group stayed (it has members, or
/// the deletion was given up halfway), to nobody if it went; then it lets go of whoever waits for it.
struct Deleting<'a> {
    groups: &'a Groups,
    group: &'a GroupId,
    /// The group's task when the deletion began, while it may still be there.
    was: Option<mpsc::Sender<Command>>,
    _lets_go: watch::Sender<()>,
}

impl Drop for Deleting<'_> {
    fn drop(&mut self) {
        let mut table = self.groups.table();
        match self.was.take() {
            Some(sender) => table.insert(self.group.clone(), Slot::Live(sender)),
            None => table.remove(self.group),
        };
        // `_lets_go` is dropped after this, with the table already as it is to be found.
    }
}

#[cfg(test)]
mod tests;
