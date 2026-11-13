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

fn one<T: TS + 'static + ?Sized>(cfg: &Config, out: &mut BTreeMap<String, String>) {
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
    groups(&cfg, &mut out);
    out
}

/// The Groups pages (R2).
fn groups(cfg: &Config, out: &mut BTreeMap<String, String>) {
    use super::groups::{
        GroupWireResponse, GroupWireRow, GroupWriteDone, GroupWriteInvalid, GroupWriteRefusal,
        GroupsResponse, OffsetDeleteRequest, ResetRequest,
    };
    use crate::lag::Lag;
    use crate::model::{
        GroupAttention, GroupDeleteView, GroupHead, GroupLagView, GroupMemberRow, GroupPart,
        GroupPhase, GroupProtocol, GroupRebalance, GroupRefused, GroupRow, GroupSpan, GroupsView,
        LagRow, LagState, LagTopic, OffsetDeleteView, RefusalStep, ResetRow, ResetView, RowLag,
        StoryCalm, StoryCause, StoryDropped, StoryFacts, StoryFlap, StoryGen, StoryHold,
        StoryJoin, StoryLast, StoryLeft, StoryNow, StoryRule, StoryTone, StoryView, StoryWho,
    };
    one::<GroupsResponse>(cfg, out);
    one::<GroupWireResponse>(cfg, out);
    one::<GroupWireRow>(cfg, out);
    one::<GroupWriteDone>(cfg, out);
    one::<GroupWriteInvalid>(cfg, out);
    one::<GroupWriteRefusal>(cfg, out);
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
