//! What goes into a consumer group and what comes out. No wire types: the handlers translate.

use std::collections::{BTreeMap, HashMap};

use kafka_wire::ResponseError;
use kafka_wire::Uuid;
use kafka_wire::ids::{InstanceId, MemberId};

use super::assign::Partitions;

/// A topic as a group needs to know it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TopicInfo {
    pub(crate) name: String,
    pub(crate) id: Uuid,
    pub(crate) partitions: i32,
}

/// The topics there are, when the group is asked something. The group never reaches for a store: whoever drives it
/// takes this snapshot and hands it in.
/// (Cheap to copy: the table is shared, so a snapshot taken for many heartbeats is one table.)
#[derive(Debug, Clone, Default)]
pub(crate) struct Topics(std::sync::Arc<Table>);

#[derive(Debug, Default)]
struct Table {
    by_name: BTreeMap<String, TopicInfo>,
    names: HashMap<Uuid, String>,
}

impl Topics {
    #[must_use]
    pub(crate) fn new(all: impl IntoIterator<Item = TopicInfo>) -> Self {
        let mut table = Table::default();
        for t in all {
            table.names.insert(t.id, t.name.clone());
            table.by_name.insert(t.name.clone(), t);
        }
        Self(std::sync::Arc::new(table))
    }

    /// The id of the topic with this name, if there is one.
    #[must_use]
    pub(crate) fn id_of(&self, name: &str) -> Option<Uuid> {
        self.0.by_name.get(name).map(|t| t.id)
    }

    /// The name of the topic with this id, if there is one.
    #[must_use]
    pub(crate) fn name_of(&self, id: &Uuid) -> Option<&str> {
        self.0.names.get(id).map(String::as_str)
    }

    /// Every topic, by name.
    pub(crate) fn all(&self) -> impl Iterator<Item = &TopicInfo> {
        self.0.by_name.values()
    }
}

/// A ConsumerGroupHeartbeat, after the checks that need nothing but the request.
#[derive(Debug, Clone)]
pub(crate) struct Heartbeat {
    /// The group's id, for the words of the answers.
    pub(crate) group: String,
    /// The member id the client has; none from a version 0 client's first request.
    pub(crate) member_id: Option<MemberId>,
    /// The version of the request.
    pub(crate) version: i16,
    /// 0 to join, a positive epoch to keep on, -1 to leave, -2 to leave and be back (a static member).
    pub(crate) epoch: i32,
    pub(crate) instance: Option<InstanceId>,
    pub(crate) rack: Option<String>,
    /// -1 if not sent.
    pub(crate) rebalance_ms: i32,
    /// Sent when it changes: `None` means unchanged.
    pub(crate) subscribed: Option<Vec<String>>,
    pub(crate) regex: Option<String>,
    pub(crate) assignor: Option<String>,
    /// What the member says it owns: `None` if it does not say this time.
    pub(crate) owned: Option<Vec<(Uuid, Vec<i32>)>>,
    pub(crate) client_id: String,
    pub(crate) client_host: String,
    /// What the caller may DESCRIBE, for the member's pattern to be held to (`None`: everything).
    pub(crate) rights: Option<crate::authorizer::TopicRights>,
}

/// The answer to a ConsumerGroupHeartbeat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeartbeatAnswer {
    /// Why not, and in what words.
    pub(crate) error: Option<(ResponseError, String)>,
    pub(crate) member_id: Option<MemberId>,
    pub(crate) epoch: i32,
    /// What the member is to own now, if that has changed from what it was last told.
    pub(crate) assignment: Option<Partitions>,
}

impl HeartbeatAnswer {
    pub(crate) fn refused(error: ResponseError, message: impl Into<String>) -> Self {
        Self {
            error: Some((error, message.into())),
            member_id: None,
            epoch: 0,
            assignment: None,
        }
    }
}

/// A member as ConsumerGroupDescribe shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemberView {
    pub(crate) id: MemberId,
    pub(crate) instance: Option<InstanceId>,
    pub(crate) rack: Option<String>,
    pub(crate) client_id: String,
    pub(crate) client_host: String,
    /// -2 for a static member that has left and will be back.
    pub(crate) epoch: i32,
    pub(crate) subscribed: Vec<String>,
    pub(crate) regex: String,
    /// What it holds, including what it has been told to give up and has not said it has.
    pub(crate) assigned: Partitions,
    pub(crate) target: Partitions,
    /// It joined by the classic protocol.
    pub(crate) classic: bool,
}

/// A consumer group as ConsumerGroupDescribe and ListGroups show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConsumerView {
    /// `Empty`, `Assigning`, `Reconciling` or `Stable`.
    pub(crate) state: &'static str,
    pub(crate) epoch: i32,
    pub(crate) assignment_epoch: i32,
    pub(crate) assignor: &'static str,
    pub(crate) members: Vec<MemberView>,
}

/// Where a group learns what topics there are, when it is asked something that needs them.
pub(crate) trait TopicSource: Send + Sync + 'static {
    fn topics(&self) -> Topics;
}
