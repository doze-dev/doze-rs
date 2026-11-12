//! View-models: what a page says, as plain data. No HTML, no broker types, nothing a page could write with.
//!
//! A page is `fn(&Model) -> Markup`. Everything that decides what the person reads (which state of Home, whether the
//! advertised address answers) is decided before, from real data, and arrives here as a value.

mod config;
mod connect;
mod consumers;
mod groups;
mod home;
pub mod live;
mod messages;
mod query;
mod story;
mod topic;
mod topics;

pub use config::{
    ConfigAsk, ConfigChange, ConfigChecked, ConfigEditView, ConfigForm, ConfigHead, ConfigInput,
    ConfigOp, ConfigPace, ConfigPage, ConfigPass, ConfigRefusal, ConfigRefused, ConfigResetResult,
    ConfigResetView, ConfigResult, ConfigRow, ConfigSaved, ConfigShow, ConfigSource, ConfigSynonym,
    ConfigTeach, ConfigValue,
};
pub use connect::{Access, ClientSnippet, ConnectView, ProbeView, Rule, Span, Tok, Verdict};
pub use consumers::{ConsumersView, TopicReader};
pub use home::{Attention, HeldBack, HomeState, HomeView, InstanceView, StopView};
pub use live::{
    ClientRow, ClientsView, FirstView, GroupWireView, LiveState, LiveView, SpokeView, WIRE_LIMIT,
    WIRE_MOST, WireClass, WireDetail, WirePart, WireQuery, WireRefusal, WireRow, WireWindow,
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

pub use story::{
    StoryCalm, StoryCause, StoryDropped, StoryFacts, StoryFlap, StoryGen, StoryHold, StoryJoin,
    StoryLast, StoryLeft, StoryNow, StoryParts, StoryRule, StoryTone, StoryView, StoryWho,
};

pub use groups::{
    GroupAttention, GroupDeleteView, GroupFilter, GroupHead, GroupLagView, GroupMemberRow,
    GroupPart, GroupPhase, GroupProtocol, GroupRebalance, GroupRefused, GroupRow, GroupSpan,
    GroupsView, LagRow, LagState, LagTopic, OffsetDeleteView, RefusalStep, ResetMode, ResetRow,
    ResetView, RowLag,
};
