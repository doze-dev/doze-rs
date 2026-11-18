//! Request handlers and the state they keep: topics, partitions and their writers, producer ids, groups,
//! transactions, the ACLs that say who may do what, and the SCRAM credentials stored over the API. No sockets here: the server hands a decoded request in and gets an [`Outcome`] back.

mod alarm;
mod authorizer;
mod blocking;
mod broker;
mod caller;
mod cleaner;
mod clock;
mod config;
mod configs;
mod data;
mod error;
mod group_configs;
mod groups;
mod guard;
mod handlers;
mod lab;
mod listener;
mod log_notes;
#[cfg(feature = "inspect")]
pub use log_notes::{DeletedNote, PassNote, RoundNote};
mod logs;
mod outcome;
mod partitions;
mod producers;
mod quotas;
mod retention;
mod share;
mod telemetry;
mod topic_config;
mod topics;
mod txn;
mod txn_index;
mod users;
#[cfg(feature = "inspect")]
mod view;

pub use broker::{Broker, Served};
pub use caller::Caller;
pub use config::{Config, Defaults, TxnVersion};
pub(crate) use configs::Live;
pub use data::{
    DataRefusal, DataUsage, Emptied, EmptiedAll, Erased, Keeps, Stray, TopicUsage, bytes_words,
    duration_words,
};
pub use error::BrokerError;
pub use groups::Cause;
pub use kafka_meta::{QuotaEntity, QuotaName};
#[cfg(feature = "inspect")]
pub use lab::{CleanOutcome, ConsoleLab, LabOutcome};
pub use listener::Listener;
pub use outcome::Outcome;
pub use quotas::{ClientQuotas, QuotaKind, Usage as QuotaUsage};
pub use topic_config::{ConfigRefused, ValueRefused, check_topic_configs, topic_config_names};
#[cfg(feature = "inspect")]
pub use view::{
    AbortedRange, AccessFacts, CleanerState, Compaction, ConfigHelp, ConfigRule, ConsoleView,
    DataSize, Durability, EventKind, GroupDetail, GroupEvent, GroupEvents, GroupKind, Instance,
    Intervals, LastStop, Limits, LogFacts, LogPolicy, LogRetention, MOST_ENTRIES, MemberDetail,
    MemberHold, OffsetExpiry, Pace, PartitionFiles, PartitionProducers, Process, ProducerBatch,
    ProducerWindow, RebalanceDetail, ShareGroupView, SharePartitionView, StreamsGroupView,
    StreamsMember, StreamsSubtopology, StreamsTasks, StreamsTopic,
};
