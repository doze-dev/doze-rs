//! The TypeScript types of the API, generated from the Rust types with `ts-rs` (a dev-dependency only).
//!
//! The test writes `console/apps/aws/src/api/types/<Type>.ts` for every type below and fails when a file it wrote was not
//! already what it wrote (a stale file), or when the directory holds a file no type writes. Run it, commit the files, rebuild
//! the bundle.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ts_rs::{Config, TS};

use crate::model::Invoked;
use crate::model::{
    About, ApiCoverage, ApiError, CallRecord, CallVia, CallsPage, CopyCommands, FlowRow,
    FlowsResponse, Outcome, RingFill, ServiceName, ServiceRow, ServicesResponse, TailReady,
};
use crate::model::{
    AccessView, Attached, CallerSeen, DataUse, DenialGroup, FlagRow, HeldPolicies, HeldPolicy,
    HomeChecks, HomeView, KeyPrincipal, LabAnswer, LabError, LabIndex, LabMember, LabOperation,
    LabTarget, PrincipalKind, PrincipalsResponse, Refusal, ResetDone, ResetRow, ServiceSeen,
    SettingsView, Signal, SignalKind, WireEdge, WireNode, Wiring,
};
use crate::model::{
    Badge, BlockerGroup, BlockerItem, Blockers, Needs, ParamKind, ParamOption, ParamSpec, Preview,
    PreviewRequest, Tag, TagsChange, TagsView, Tone, VerbSpec, Wired,
};
use crate::model::{
    ColumnSpec, CreateSpec, Created, DeepTab, DeleteSpec, Deleted, FieldValue, FormInput,
    KindCount, ResourceDetail, ResourceLink, ResourceList, ResourceRow, ResourceService,
    ResourcesIndex, Revealed, SetAside, SubAdd, SubList, Uploaded, ValueFormat,
};
use crate::model::{RegionRow, RegionsView};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../console/apps/aws/src/api/types")
}

fn config() -> Config {
    // u64 is a JSON number: every value the console sends is far under 2^53 (times in ms, counts, bytes).
    Config::new().with_large_int("number").with_out_dir(dir())
}

fn one<T: TS + 'static + ?Sized>(cfg: &Config, out: &mut BTreeMap<String, String>) {
    let name = T::name(cfg);
    let text = T::export_to_string(cfg).unwrap_or_else(|e| panic!("{name}: {e}"));
    out.insert(format!("{name}.ts"), text);
}

/// Every type the API sends, by file name.
#[allow(clippy::too_many_lines)] // one line a type
fn all() -> BTreeMap<String, String> {
    let cfg = config();
    let mut out = BTreeMap::new();
    one::<ApiError>(&cfg, &mut out);
    one::<Invoked>(&cfg, &mut out);
    crate::api::lab_once::register(&cfg, &mut out);
    one::<About>(&cfg, &mut out);
    one::<ServiceName>(&cfg, &mut out);
    one::<ServicesResponse>(&cfg, &mut out);
    one::<ServiceRow>(&cfg, &mut out);
    one::<ApiCoverage>(&cfg, &mut out);
    one::<CallsPage>(&cfg, &mut out);
    one::<CallRecord>(&cfg, &mut out);
    one::<Outcome>(&cfg, &mut out);
    one::<RingFill>(&cfg, &mut out);
    one::<TailReady>(&cfg, &mut out);
    one::<CallVia>(&cfg, &mut out);
    one::<FlowRow>(&cfg, &mut out);
    one::<FlowsResponse>(&cfg, &mut out);
    one::<CopyCommands>(&cfg, &mut out);
    one::<ResourcesIndex>(&cfg, &mut out);
    one::<RegionsView>(&cfg, &mut out);
    one::<RegionRow>(&cfg, &mut out);
    one::<ResourceService>(&cfg, &mut out);
    one::<KindCount>(&cfg, &mut out);
    one::<ValueFormat>(&cfg, &mut out);
    one::<ColumnSpec>(&cfg, &mut out);
    one::<ResourceRow>(&cfg, &mut out);
    one::<ResourceLink>(&cfg, &mut out);
    one::<FormInput>(&cfg, &mut out);
    one::<CreateSpec>(&cfg, &mut out);
    one::<DeleteSpec>(&cfg, &mut out);
    one::<ResourceList>(&cfg, &mut out);
    one::<FieldValue>(&cfg, &mut out);
    one::<SetAside>(&cfg, &mut out);
    one::<SubAdd>(&cfg, &mut out);
    one::<SubList>(&cfg, &mut out);
    one::<ResourceDetail>(&cfg, &mut out);
    one::<Created>(&cfg, &mut out);
    one::<Revealed>(&cfg, &mut out);
    one::<Deleted>(&cfg, &mut out);
    one::<DeepTab>(&cfg, &mut out);
    one::<Uploaded>(&cfg, &mut out);
    one::<crate::api::objects::Presigned>(&cfg, &mut out);
    one::<KeyPrincipal>(&cfg, &mut out);
    one::<PrincipalKind>(&cfg, &mut out);
    one::<PrincipalsResponse>(&cfg, &mut out);
    one::<Refusal>(&cfg, &mut out);
    one::<DenialGroup>(&cfg, &mut out);
    one::<AccessView>(&cfg, &mut out);
    one::<Attached>(&cfg, &mut out);
    one::<CallerSeen>(&cfg, &mut out);
    one::<ServiceSeen>(&cfg, &mut out);
    one::<SignalKind>(&cfg, &mut out);
    one::<Signal>(&cfg, &mut out);
    one::<HomeView>(&cfg, &mut out);
    one::<HomeChecks>(&cfg, &mut out);
    one::<HeldPolicy>(&cfg, &mut out);
    one::<HeldPolicies>(&cfg, &mut out);
    one::<WireNode>(&cfg, &mut out);
    one::<WireEdge>(&cfg, &mut out);
    one::<Wiring>(&cfg, &mut out);
    one::<FlagRow>(&cfg, &mut out);
    one::<DataUse>(&cfg, &mut out);
    one::<SettingsView>(&cfg, &mut out);
    one::<ResetRow>(&cfg, &mut out);
    one::<ResetDone>(&cfg, &mut out);
    one::<LabTarget>(&cfg, &mut out);
    one::<LabIndex>(&cfg, &mut out);
    one::<LabMember>(&cfg, &mut out);
    one::<LabOperation>(&cfg, &mut out);
    one::<LabError>(&cfg, &mut out);
    one::<LabAnswer>(&cfg, &mut out);
    page_types(&cfg, &mut out);
    lab_types(&cfg, &mut out);
    out
}

/// What the Lab's fault rules send and take.
fn lab_types(cfg: &Config, out: &mut BTreeMap<String, String>) {
    use crate::model::{
        LabCheck, LabCleared, LabCode, LabDid, LabEffect, LabEnd, LabErrors, LabFailKind, LabFirst,
        LabGuide, LabIntro, LabKind, LabLimits, LabListedError, LabMade, LabMatcher, LabProblem,
        LabRule, LabRuleBody, LabRuleView, LabRules, LabSetting, LabShare, LabStep, LabStopped,
        LabSwitchBody, LabTag, LabTeach,
    };
    one::<LabKind>(cfg, out);
    one::<LabFailKind>(cfg, out);
    one::<LabEffect>(cfg, out);
    one::<LabShare>(cfg, out);
    one::<LabMatcher>(cfg, out);
    one::<LabRuleBody>(cfg, out);
    one::<LabSwitchBody>(cfg, out);
    one::<LabProblem>(cfg, out);
    one::<LabCode>(cfg, out);
    one::<LabCheck>(cfg, out);
    one::<LabMade>(cfg, out);
    one::<LabEnd>(cfg, out);
    one::<LabFirst>(cfg, out);
    one::<LabRule>(cfg, out);
    one::<LabLimits>(cfg, out);
    one::<LabRules>(cfg, out);
    one::<LabRuleView>(cfg, out);
    one::<LabStopped>(cfg, out);
    one::<LabCleared>(cfg, out);
    one::<LabListedError>(cfg, out);
    one::<LabErrors>(cfg, out);
    one::<LabDid>(cfg, out);
    one::<LabTag>(cfg, out);
    one::<LabStep>(cfg, out);
    one::<LabIntro>(cfg, out);
    one::<LabSetting>(cfg, out);
    one::<LabTeach>(cfg, out);
    one::<LabGuide>(cfg, out);
}

/// What a resource page adds: a call's preview, tags, what stands in the way of a delete, a list's parameters, and how a
/// detail speaks of its resource.
fn page_types(cfg: &Config, out: &mut BTreeMap<String, String>) {
    one::<PreviewRequest>(cfg, out);
    one::<Preview>(cfg, out);
    one::<Tag>(cfg, out);
    one::<TagsView>(cfg, out);
    one::<TagsChange>(cfg, out);
    one::<BlockerItem>(cfg, out);
    one::<BlockerGroup>(cfg, out);
    one::<Blockers>(cfg, out);
    one::<ParamKind>(cfg, out);
    one::<ParamOption>(cfg, out);
    one::<ParamSpec>(cfg, out);
    one::<Tone>(cfg, out);
    one::<Badge>(cfg, out);
    one::<Needs>(cfg, out);
    one::<VerbSpec>(cfg, out);
    one::<Wired>(cfg, out);
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
        "console/apps/aws/src/api/types was stale: rewrote {stale:?}; no type writes {extra:?} (delete them). Commit the files and rebuild the bundle."
    );
}

#[test]
fn an_outcome_is_sent_as_one_lower_case_word() {
    for (outcome, word) in [
        (Outcome::Ok, "\"ok\""),
        (Outcome::Error, "\"error\""),
        (Outcome::Denied, "\"denied\""),
        (Outcome::NotBuilt, "\"not-built\""),
    ] {
        assert_eq!(serde_json::to_string(&outcome).expect("json"), word);
    }
}

#[test]
fn an_outcome_tells_iam_saying_no_from_doze_not_doing_it_from_any_other_error() {
    assert_eq!(Outcome::of(200, ""), Outcome::Ok);
    assert_eq!(Outcome::of(304, ""), Outcome::Ok);
    assert_eq!(Outcome::of(403, "AccessDenied"), Outcome::Denied);
    assert_eq!(Outcome::of(400, "AccessDeniedException"), Outcome::Denied);
    assert_eq!(Outcome::of(501, "NotImplemented"), Outcome::NotBuilt);
    assert_eq!(Outcome::of(403, "OptInRequired"), Outcome::NotBuilt);
    // A validation error is an error like any other.
    assert_eq!(Outcome::of(400, "ValidationException"), Outcome::Error);
    assert_eq!(Outcome::of(400, "QueueDoesNotExist"), Outcome::Error);
    assert_eq!(Outcome::of(500, "InternalFailure"), Outcome::Error);
    assert_eq!(Outcome::of(403, ""), Outcome::Error);
}

#[test]
fn a_dropped_call_is_an_error_and_a_denial_in_snss_words_is_a_denial() {
    // A connection a Lab rule closed has no status; nobody said no, and it did not succeed.
    assert_eq!(Outcome::of(0, ""), Outcome::Error);
    assert_eq!(Outcome::of(403, "AuthorizationError"), Outcome::Denied);
    assert_eq!(Outcome::of(403, "AccessDenied"), Outcome::Denied);
    assert_eq!(Outcome::of(429, "Throttled"), Outcome::Error);
}
