//! A streams group (KIP-1071) as the console shows it: what StreamsGroupDescribe says, and what it does not (why the
//! topology is not ready, each member's reconciliation stage and the tasks it is giving up, how many tasks each
//! subtopology has).
//!
//! A group in memory is described as it is now; one not in memory (an empty group from before the broker started again)
//! from its kept record, as StreamsGroupDescribe describes it.

use kafka_auth::authz::{Operation, ResourceType};
use kafka_storage::fs::Fs;
use kafka_wire::ids::GroupId;

use super::ConsoleView;
use crate::Caller;
use crate::groups::streams::{
    Stage, StreamsGroup, StreamsMemberView, StreamsView, Task, Tasks, topology,
};

/// A topic a subtopology names, with what the topology says of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamsTopic {
    pub name: String,
    /// As worked out once the topology is ready; 0 while it is not.
    pub partitions: i32,
    /// 0 or -1: the broker's default.
    pub replication: i16,
    pub configs: Vec<(String, String)>,
}

/// One subtopology: the topics it reads and writes, and its tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamsSubtopology {
    pub id: String,
    pub sources: Vec<String>,
    pub repartition_sinks: Vec<String>,
    pub repartition_sources: Vec<StreamsTopic>,
    pub changelogs: Vec<StreamsTopic>,
    /// How many tasks it has; `None` while the topology is not ready.
    pub tasks: Option<i32>,
}

/// Tasks by role, each as `(subtopology, partition)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StreamsTasks {
    pub active: Vec<(String, i32)>,
    pub standby: Vec<(String, i32)>,
    pub warmup: Vec<(String, i32)>,
}

/// One member of a streams group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamsMember {
    pub id: String,
    pub epoch: i32,
    pub instance: Option<String>,
    pub rack: Option<String>,
    pub client_id: String,
    pub client_host: String,
    pub topology_epoch: i32,
    /// The process (one Kafka Streams instance) it runs in.
    pub process: String,
    /// `host:port` of its interactive queries, if it said one.
    pub endpoint: Option<String>,
    pub client_tags: Vec<(String, String)>,
    /// What it runs now.
    pub assignment: StreamsTasks,
    /// What it is to run.
    pub target: StreamsTasks,
    /// What it was told to give up and has not said it has.
    pub revoking: StreamsTasks,
    /// `STABLE`, `UNREVOKED_TASKS` or `UNRELEASED_TASKS`.
    pub stage: &'static str,
    /// The offsets its tasks have reached and where their changelogs end, as it last said (task to offset).
    pub task_offsets: Vec<((String, i32), i64)>,
    pub task_end_offsets: Vec<((String, i32), i64)>,
}

/// A streams group, whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamsGroupView {
    /// `Empty`, `NotReady`, `Assigning`, `Reconciling` or `Stable`.
    pub state: &'static str,
    pub epoch: i32,
    pub assignment_epoch: i32,
    /// The topology's epoch; `None` before a member sent one.
    pub topology_epoch: Option<i32>,
    /// Why the topology is not ready: the status code the members are told and its words.
    pub not_ready: Option<(i8, String)>,
    pub subtopologies: Vec<StreamsSubtopology>,
    pub members: Vec<StreamsMember>,
    /// Whether the broker has the group in memory (else it is described from its kept record, with no member).
    pub live: bool,
}

fn topic(t: &topology::TopicInfo) -> StreamsTopic {
    StreamsTopic {
        name: t.name.clone(),
        partitions: t.partitions,
        replication: t.replication,
        configs: t.configs.clone(),
    }
}

fn tasks(t: &Tasks) -> StreamsTasks {
    let list = |s: &std::collections::BTreeSet<Task>| s.iter().cloned().collect();
    StreamsTasks {
        active: list(&t.active),
        standby: list(&t.standby),
        warmup: list(&t.warmup),
    }
}

fn member(m: &StreamsMemberView) -> StreamsMember {
    StreamsMember {
        id: m.id.as_str().to_owned(),
        epoch: m.epoch,
        instance: m.instance.as_ref().map(|i| i.as_str().to_owned()),
        rack: m.rack.clone(),
        client_id: m.client_id.clone(),
        client_host: m.client_host.clone(),
        topology_epoch: m.topology_epoch,
        process: m.process.clone(),
        endpoint: m
            .endpoint
            .as_ref()
            .map(|e| format!("{}:{}", e.host, e.port)),
        client_tags: m
            .client_tags
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        assignment: tasks(&m.assignment),
        target: tasks(&m.target),
        revoking: tasks(&m.revoking),
        stage: Stage::name(m.stage),
        task_offsets: m.offsets.iter().map(|(t, o)| (t.clone(), *o)).collect(),
        task_end_offsets: m.end_offsets.iter().map(|(t, o)| (t.clone(), *o)).collect(),
    }
}

fn of(v: &StreamsView, live: bool) -> StreamsGroupView {
    let subtopologies = v
        .topology
        .iter()
        .flat_map(|t| &t.subtopologies)
        .map(|(id, s)| StreamsSubtopology {
            id: id.clone(),
            sources: s.sources.clone(),
            repartition_sinks: s.repartition_sinks.clone(),
            repartition_sources: s.repartition_sources.iter().map(topic).collect(),
            changelogs: s.changelogs.iter().map(topic).collect(),
            tasks: v.tasks.get(id).copied(),
        })
        .collect();
    StreamsGroupView {
        state: v.state,
        epoch: v.epoch,
        assignment_epoch: v.assignment_epoch,
        topology_epoch: v.topology.as_ref().map(|t| t.epoch),
        not_ready: v.not_ready.clone(),
        subtopologies,
        members: v.members.iter().map(member).collect(),
        live,
    }
}

/// Every topic a group's topology names.
fn named(v: &StreamsGroupView) -> impl Iterator<Item = &str> {
    v.subtopologies.iter().flat_map(|s| {
        s.sources
            .iter()
            .chain(&s.repartition_sinks)
            .map(String::as_str)
            .chain(
                s.repartition_sources
                    .iter()
                    .chain(&s.changelogs)
                    .map(|t| t.name.as_str()),
            )
    })
}

impl<F: Fs> ConsoleView<'_, F> {
    /// The streams group `group`, if `caller` may DESCRIBE it and every topic its topology names (StreamsGroupDescribe
    /// refuses a group whose topology names a topic the caller may not see); `None` too when it is no streams group.
    pub async fn streams_group(&self, caller: &Caller, group: &str) -> Option<StreamsGroupView> {
        let b = self.broker;
        if !b
            .authz
            .allows(caller, ResourceType::Group, group, Operation::Describe)
        {
            return None;
        }
        let id = GroupId::parse(group).ok()?;
        let view = match b.groups.describe(&id).await {
            Some(d) => of(d.streams.as_deref()?, true),
            None => {
                let life = crate::blocking::run(|| b.meta.group_life(&id)).ok()??;
                let kept = crate::groups::streams::stored::kept_of(life.streams.as_ref()?);
                of(&StreamsGroup::restored(kept).view(), false)
            }
        };
        let seen = named(&view).all(|t| b.topic_allows(caller, t, Operation::Describe));
        seen.then_some(view)
    }
}
