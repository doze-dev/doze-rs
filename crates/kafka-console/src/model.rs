//! View-models: what a page says, as plain data. No HTML, no broker types, nothing a page could write with.
//!
//! A page is `fn(&Model) -> Markup`. Everything that decides what the person reads (which state of Home, whether the
//! advertised address answers) is decided before, from real data, and arrives here as a value.

mod config;
mod connect;
mod groups;
mod home;
mod log;
mod messages;
mod producers;
mod query;
mod topic;
mod topics;

pub use config::{
    ConfigAsk, ConfigChange, ConfigChecked, ConfigEditView, ConfigForm, ConfigHead, ConfigInput,
    ConfigOp, ConfigPace, ConfigPage, ConfigPass, ConfigRefusal, ConfigRefused, ConfigResetResult,
    ConfigResetView, ConfigResult, ConfigRow, ConfigSaved, ConfigShow, ConfigSource, ConfigSynonym,
    ConfigTeach, ConfigValue,
};
pub use connect::{Access, ClientSnippet, ConnectView, ProbeView, Rule, Span, Tok, Verdict};
pub use home::{Attention, HeldBack, HomeState, HomeView, InstanceView, StopView};
pub use log::{
    LogCleaner, LogCompaction, LogDurability, LogLimit, LogMoved, LogOther, LogOtherLink, LogPass,
    LogPlanner, LogRetentionView, LogRound, LogRules, LogSegment, LogState, LogStop, LogTombstone,
    LogView,
};
pub use producers::{
    ProdAborted, ProdBatch, ProdCell, ProdRow, ProdTail, ProdTxn, ProducersView, TxnPart, TxnRow,
    TxnView,
};

pub(crate) mod write;

pub use messages::{
    HeaderRow, Hold, Kind, MessagesView, Reach, RecordRow, RecordTxn, RecordView, Shown, Standing,
};
pub use query::{DEFAULT_LIMIT, MOST_KEY, MOST_LIMIT, MessageQuery, Reading};
pub use topic::{CallVersions, OpenTxn, PartitionRow, TabCounts, TopicHead};
pub use topics::{Cleanup, TopicRow, TopicsState, TopicsView};
pub use write::{
    AddView, Checked, CreateRefused, CreateResult, CreateView, Created, DeleteRecordsRefused,
    DeleteRecordsView, DeleteView, Deleted, Draft, Errors, InUse, Lines, ProduceErrors,
    ProduceInput, ProduceRefused, ProduceResult, ProduceView, Produced, RecordsDeleted, Refusal,
    Remaining, Step, Teach, Template, Visibility,
};

pub use groups::{
    GroupAttention, GroupDeleteView, GroupFilter, GroupHead, GroupLagView, GroupMemberRow,
    GroupPart, GroupPhase, GroupProtocol, GroupRebalance, GroupRefused, GroupRow, GroupSpan,
    GroupsView, LagRow, LagState, LagTopic, OffsetDeleteView, RefusalStep, ResetMode, ResetRow,
    ResetView, RowLag,
};
