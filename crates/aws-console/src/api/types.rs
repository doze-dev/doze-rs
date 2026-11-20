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
    ColumnSpec, CreateSpec, Created, DeepTab, DeleteSpec, Deleted, FieldValue, FormInput,
    KindCount, ResourceDetail, ResourceLink, ResourceList, ResourceRow, ResourceService,
    ResourcesIndex, Revealed, SetAside, SubAdd, SubList, Uploaded, ValueFormat,
};

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
fn all() -> BTreeMap<String, String> {
    let cfg = config();
    let mut out = BTreeMap::new();
    one::<ApiError>(&cfg, &mut out);
    one::<Invoked>(&cfg, &mut out);
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
    out
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
