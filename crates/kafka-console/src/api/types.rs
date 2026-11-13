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
    live_types(&cfg, &mut out);
    out
}

/// The Live types (slice R3).
fn live_types(cfg: &Config, out: &mut BTreeMap<String, String>) {
    use super::live::{
        FetchCopy, FirstResponse, LiveResponse, LiveWords, RequestResponse, TailBatch, TailReady,
        TopicWireView,
    };
    use crate::model::{
        ClientRow, ClientsView, Facet, Facets, FirstView, GroupWireView,
        LiveState, LiveView, WireClass, WireDetail, WirePart, WireQuery, WireRefusal, WireRow,
        WireWindow,
    };
    use crate::model::live::{LabDid, LabTag};
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
