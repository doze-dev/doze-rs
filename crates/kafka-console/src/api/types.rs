//! The TypeScript types of the API, generated from the Rust types with `ts-rs` (a dev-dependency only).
//!
//! The test writes `web/src/api/types/<Type>.ts` for every type below and fails when a file it wrote was not already what it
//! wrote (a stale file), or when the directory holds a file no type writes. Run it, commit the files, rebuild the bundle.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ts_rs::{Config, TS};

use super::{
    ApiError, ConnectResponse, ConnectWords, HomeResponse, HomeWords, JumpView, ShellView,
};
use crate::model::{
    Access, Attention, ClientSnippet, ConnectView, HeldBack, HomeState, HomeView, InstanceView,
    ProbeView, Rule, Span, SpokeView, StopView, Tok, Verdict,
};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/src/api/types")
}

fn config() -> Config {
    // i64 and u64 are JSON numbers: every value the console sends is far under 2^53 (offsets, times in ms, counts).
    Config::new().with_large_int("number").with_out_dir(dir())
}

pub(super) fn one<T: TS + 'static + ?Sized>(cfg: &Config, out: &mut BTreeMap<String, String>) {
    let name = T::name(cfg);
    let text = T::export_to_string(cfg).unwrap_or_else(|e| panic!("{name}: {e}"));
    out.insert(format!("{name}.ts"), text);
}

/// Every type the API sends, by file name.
fn all() -> BTreeMap<String, String> {
    let cfg = config();
    let mut out = BTreeMap::new();
    one::<ApiError>(&cfg, &mut out);
    one::<HomeResponse>(&cfg, &mut out);
    one::<HomeWords>(&cfg, &mut out);
    one::<HomeView>(&cfg, &mut out);
    one::<HomeState>(&cfg, &mut out);
    one::<Attention>(&cfg, &mut out);
    one::<HeldBack>(&cfg, &mut out);
    one::<InstanceView>(&cfg, &mut out);
    one::<StopView>(&cfg, &mut out);
    one::<SpokeView>(&cfg, &mut out);
    one::<ConnectResponse>(&cfg, &mut out);
    one::<ConnectWords>(&cfg, &mut out);
    one::<ConnectView>(&cfg, &mut out);
    one::<ProbeView>(&cfg, &mut out);
    one::<Rule>(&cfg, &mut out);
    one::<Verdict>(&cfg, &mut out);
    one::<Span>(&cfg, &mut out);
    one::<Access>(&cfg, &mut out);
    one::<ClientSnippet>(&cfg, &mut out);
    one::<Tok>(&cfg, &mut out);
    one::<ShellView>(&cfg, &mut out);
    one::<JumpView>(&cfg, &mut out);
    r1b(&cfg, &mut out);
    groups(&cfg, &mut out);
    live_types(&cfg, &mut out);
    super::topics::types::all(&cfg, &mut out);
    out
}

/// Slice R1b: the topic writes and the Config tab.
fn r1b(cfg: &Config, out: &mut BTreeMap<String, String>) {
    use super::topic_admin::*;
    use super::topic_writes::*;
    use crate::model::write::{RecordRow, TopicHead, Visibility};
    use crate::model::*;
    one::<CreateForm>(cfg, out);
    one::<CreatedAnswer>(cfg, out);
    one::<CheckedAnswer>(cfg, out);
    one::<ProduceForm>(cfg, out);
    one::<ProducedAnswer>(cfg, out);
    one::<ProduceRefusedAnswer>(cfg, out);
    one::<AddForm>(cfg, out);
    one::<AddedAnswer>(cfg, out);
    one::<RecordsForm>(cfg, out);
    one::<RecordsRefusedAnswer>(cfg, out);
    one::<RecordsDeletedAnswer>(cfg, out);
    one::<DeleteForm>(cfg, out);
    one::<DeletedAnswer>(cfg, out);
    one::<CreateView>(cfg, out);
    one::<Draft>(cfg, out);
    one::<Step>(cfg, out);
    one::<Template>(cfg, out);
    one::<Errors>(cfg, out);
    one::<Teach>(cfg, out);
    one::<Checked>(cfg, out);
    one::<Created>(cfg, out);
    one::<CreateRefused>(cfg, out);
    one::<Refusal>(cfg, out);
    one::<ProduceView>(cfg, out);
    one::<ProduceErrors>(cfg, out);
    one::<Produced>(cfg, out);
    one::<TopicHead>(cfg, out);
    one::<RecordRow>(cfg, out);
    one::<Visibility>(cfg, out);
    one::<ProduceRefused>(cfg, out);
    one::<AddView>(cfg, out);
    one::<DeleteRecordsView>(cfg, out);
    one::<DeleteRecordsRefused>(cfg, out);
    one::<InUse>(cfg, out);
    one::<DeleteView>(cfg, out);
    one::<Deleted>(cfg, out);
    one::<Remaining>(cfg, out);
    r1b_config(cfg, out);
}

/// Slice R1b: the Config tab.
fn r1b_config(cfg: &Config, out: &mut BTreeMap<String, String>) {
    use super::config::*;
    use super::config_edit::*;
    use crate::model::*;
    one::<ConfigPlace>(cfg, out);
    one::<ConfigRowView>(cfg, out);
    one::<ConfigGroup>(cfg, out);
    one::<ConfigTab>(cfg, out);
    one::<ConfigEditForm>(cfg, out);
    one::<ConfigSavedAnswer>(cfg, out);
    one::<ConfigEditAnswer>(cfg, out);
    one::<ConfigRefusedAnswer>(cfg, out);
    one::<ConfigResetForm>(cfg, out);
    one::<ConfigResetDone>(cfg, out);
    one::<Cleanup>(cfg, out);
    one::<ConfigSource>(cfg, out);
    one::<ConfigOp>(cfg, out);
    one::<ConfigForm>(cfg, out);
    one::<ConfigEditView>(cfg, out);
    one::<ConfigRow>(cfg, out);
    one::<ConfigSynonym>(cfg, out);
    one::<ConfigInput>(cfg, out);
    one::<ConfigPace>(cfg, out);
    one::<ConfigTeach>(cfg, out);
    one::<ConfigChecked>(cfg, out);
    one::<ConfigSaved>(cfg, out);
    one::<ConfigHead>(cfg, out);
    one::<ConfigChange>(cfg, out);
    one::<ConfigValue>(cfg, out);
    one::<ConfigRefused>(cfg, out);
    one::<ConfigRefusal>(cfg, out);
    one::<ConfigPass>(cfg, out);
    one::<ConfigResetView>(cfg, out);
}

/// The Groups pages (R2).
fn groups(cfg: &Config, out: &mut BTreeMap<String, String>) {
    use super::groups::{GroupWriteOutcome, GroupsResponse, OffsetDeleteRequest, ResetRequest};
    use crate::lag::Lag;
    use crate::model::{
        GroupAttention, GroupDeleteView, GroupHead, GroupLagView, GroupMemberRow, GroupPart,
        GroupPhase, GroupProtocol, GroupRebalance, GroupRefused, GroupRow, GroupSpan, GroupsView,
        LagRow, LagState, LagTopic, OffsetDeleteView, RefusalStep, ResetRow, ResetView, RowLag,
        StoryCalm, StoryCause, StoryDropped, StoryFacts, StoryFlap, StoryGen, StoryHold, StoryJoin,
        StoryLast, StoryLeft, StoryNow, StoryRule, StoryTone, StoryView, StoryWho,
    };
    one::<GroupsResponse>(cfg, out);
    one::<GroupWriteOutcome>(cfg, out);
    one::<OffsetDeleteRequest>(cfg, out);
    one::<ResetRequest>(cfg, out);
    one::<Lag>(cfg, out);
    one::<GroupAttention>(cfg, out);
    one::<GroupDeleteView>(cfg, out);
    one::<GroupHead>(cfg, out);
    one::<GroupLagView>(cfg, out);
    one::<GroupMemberRow>(cfg, out);
    one::<GroupPart>(cfg, out);
    one::<GroupPhase>(cfg, out);
    one::<GroupProtocol>(cfg, out);
    one::<GroupRebalance>(cfg, out);
    one::<GroupRefused>(cfg, out);
    one::<GroupRow>(cfg, out);
    one::<GroupSpan>(cfg, out);
    one::<GroupsView>(cfg, out);
    one::<LagRow>(cfg, out);
    one::<LagState>(cfg, out);
    one::<LagTopic>(cfg, out);
    one::<OffsetDeleteView>(cfg, out);
    one::<RefusalStep>(cfg, out);
    one::<ResetRow>(cfg, out);
    one::<ResetView>(cfg, out);
    one::<RowLag>(cfg, out);
    one::<StoryCalm>(cfg, out);
    one::<StoryCause>(cfg, out);
    one::<StoryDropped>(cfg, out);
    one::<StoryFacts>(cfg, out);
    one::<StoryFlap>(cfg, out);
    one::<StoryGen>(cfg, out);
    one::<StoryHold>(cfg, out);
    one::<StoryJoin>(cfg, out);
    one::<StoryLast>(cfg, out);
    one::<StoryLeft>(cfg, out);
    one::<StoryNow>(cfg, out);
    one::<StoryRule>(cfg, out);
    one::<StoryTone>(cfg, out);
    one::<StoryView>(cfg, out);
    one::<StoryWho>(cfg, out);
}

/// The Live types (slice R3).
fn live_types(cfg: &Config, out: &mut BTreeMap<String, String>) {
    use super::live::{
        FetchCopy, FirstResponse, LiveResponse, LiveWords, RequestAnswer, RequestResponse,
        TailBatch, TailReady, TopicWireView,
    };
    use crate::model::live::{LabDid, LabTag};
    use crate::model::{
        ClientRow, ClientsView, Facet, Facets, FirstView, GroupWireView, LiveState, LiveView,
        WireClass, WireDetail, WirePart, WireQuery, WireRefusal, WireRow, WireWindow,
    };
    use crate::pages::live::{RequestKind, RequestLimits, RequestRule};
    one::<LiveResponse>(cfg, out);
    one::<LiveWords>(cfg, out);
    one::<LiveView>(cfg, out);
    one::<LiveState>(cfg, out);
    one::<WireRow>(cfg, out);
    one::<WireClass>(cfg, out);
    one::<LabTag>(cfg, out);
    one::<LabDid>(cfg, out);
    one::<WireQuery>(cfg, out);
    one::<WireWindow>(cfg, out);
    one::<WireRefusal>(cfg, out);
    one::<Facet>(cfg, out);
    one::<Facets>(cfg, out);
    one::<WirePart>(cfg, out);
    one::<WireDetail>(cfg, out);
    one::<RequestResponse>(cfg, out);
    one::<RequestAnswer>(cfg, out);
    one::<RequestKind>(cfg, out);
    one::<RequestLimits>(cfg, out);
    one::<RequestRule>(cfg, out);
    one::<FetchCopy>(cfg, out);
    one::<ClientsView>(cfg, out);
    one::<ClientRow>(cfg, out);
    one::<FirstResponse>(cfg, out);
    one::<FirstView>(cfg, out);
    one::<TopicWireView>(cfg, out);
    one::<GroupWireView>(cfg, out);
    one::<TailBatch>(cfg, out);
    one::<TailReady>(cfg, out);
}

#[test]
fn the_typescript_types_of_the_api_are_current() {
    let dir = dir();
    std::fs::create_dir_all(&dir).expect("the types directory");
    let want = all();
    let mut stale = Vec::new();
    for (name, text) in &want {
        let path = dir.join(name);
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        if have != *text {
            std::fs::write(&path, text).expect("write a type");
            stale.push(name.clone());
        }
    }
    let mut extra = Vec::new();
    for entry in std::fs::read_dir(&dir)
        .expect("read the types directory")
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".ts") && !want.contains_key(&name) {
            extra.push(name);
        }
    }
    assert!(
        stale.is_empty() && extra.is_empty(),
        "web/src/api/types was stale: rewrote {stale:?}; no type writes {extra:?} (delete them). Commit the files and run `bun run build` in web/."
    );
}

#[test]
fn a_home_answer_carries_no_memory_figure() {
    let home = crate::fixtures::home(HomeState::Steady);
    let text = serde_json::to_string(&super::HomeResponse::new(home)).expect("json");
    assert!(!text.contains("resident"), "{text}");
    assert!(text.contains("\"kind\":\"steady\""), "{text}");
}
