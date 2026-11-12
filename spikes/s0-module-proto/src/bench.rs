//! Codec comparison on the same data: minicbor (numbered fields), postcard (serde,
//! positional), prost (hand-annotated). Sizes, encode+decode time, and what happens
//! when the two sides were built from different versions of a message.

use anyhow::Result;
use minicbor::{Decode, Encode};
use prost::Message;
use s0_module_proto::{msg, pb};
use serde::{Deserialize, Serialize};
use std::hint::black_box;
use std::time::Instant;

fn time_ns(iters: u32, mut f: impl FnMut()) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t0 = Instant::now();
        for _ in 0..iters {
            f();
        }
        best = best.min(t0.elapsed().as_nanos() as f64 / f64::from(iters));
    }
    best
}

fn row(name: &str, cbor: (usize, f64), pc: (usize, f64), pbv: (usize, f64)) {
    println!(
        "{name:<10} bytes cbor={:<5} postcard={:<5} prost={:<5}  enc+dec ns cbor={:<7.0} postcard={:<7.0} prost={:<7.0}",
        cbor.0, pc.0, pbv.0, cbor.1, pc.1, pbv.1
    );
}

pub fn run() -> Result<()> {
    let req = msg::sample_plan_req();
    let plan = msg::sample_spawn_plan();
    let preq = pb::plan_req(&req);
    let pplan = pb::spawn_plan(&plan);
    let n = 20_000;

    let c = minicbor::to_vec(&req)?;
    let p = postcard::to_stdvec(&req)?;
    let b = preq.encode_to_vec();
    let tc = time_ns(n, || {
        let v = minicbor::to_vec(black_box(&req)).unwrap_or_default();
        let d: msg::PlanReq = minicbor::decode(&v).expect("cbor");
        black_box(d);
    });
    let tp = time_ns(n, || {
        let v = postcard::to_stdvec(black_box(&req)).unwrap_or_default();
        let d: msg::PlanReq = postcard::from_bytes(&v).expect("postcard");
        black_box(d);
    });
    let tb = time_ns(n, || {
        let v = black_box(&preq).encode_to_vec();
        let d = pb::PlanReq::decode(v.as_slice()).expect("prost");
        black_box(d);
    });
    row("PlanReq", (c.len(), tc), (p.len(), tp), (b.len(), tb));

    let c = minicbor::to_vec(&plan)?;
    let p = postcard::to_stdvec(&plan)?;
    let b = pplan.encode_to_vec();
    let tc = time_ns(n, || {
        let v = minicbor::to_vec(black_box(&plan)).unwrap_or_default();
        black_box(minicbor::decode::<msg::SpawnPlan>(&v).expect("cbor"));
    });
    let tp = time_ns(n, || {
        let v = postcard::to_stdvec(black_box(&plan)).unwrap_or_default();
        black_box(postcard::from_bytes::<msg::SpawnPlan>(&v).expect("pc"));
    });
    let tb = time_ns(n, || {
        let v = black_box(&pplan).encode_to_vec();
        black_box(pb::SpawnPlan::decode(v.as_slice()).expect("pb"));
    });
    row("SpawnPlan", (c.len(), tc), (p.len(), tp), (b.len(), tb));

    let hello = msg::sample_hello();
    println!("Hello      bytes cbor={} postcard={}", minicbor::to_vec(&hello)?.len(), postcard::to_stdvec(&hello)?.len());

    evolution()
}

// ---- version skew -------------------------------------------------------------------
// v1: what an older build knows. v2: a newer build added `term` and `grace_ms` to the
// end of a spec and a new Value variant. Each case: does decoding still give the right
// answer?

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
struct SpecV1 {
    #[n(0)] name: String,
    #[n(1)] args: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
struct SpecV2 {
    #[n(0)] name: String,
    #[n(1)] args: Vec<String>,
    #[n(2)] #[cbor(default)] #[serde(default)] term: bool,
    #[n(3)] grace_ms: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
struct PlanV1 {
    #[n(0)] specs: Vec<SpecV1>,
    #[n(1)] note: String,
}
#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
struct PlanV2 {
    #[n(0)] specs: Vec<SpecV2>,
    #[n(1)] note: String,
}
#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
enum KindV1 {
    #[n(0)] Tcp,
    #[n(1)] Http,
}
#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
enum KindV2 {
    #[n(0)] Tcp,
    #[n(1)] Http,
    #[n(2)] Grpc,
}
#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
struct ListenerV1 {
    #[n(0)] name: String,
    #[n(1)] kind: Option<KindV1>,
}
#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
struct ListenerV2 {
    #[n(0)] name: String,
    #[n(1)] kind: Option<KindV2>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct PSpecV1 {
    #[prost(string, tag = "1")] name: String,
    #[prost(string, repeated, tag = "2")] args: Vec<String>,
}
#[derive(Clone, PartialEq, prost::Message)]
struct PSpecV2 {
    #[prost(string, tag = "1")] name: String,
    #[prost(string, repeated, tag = "2")] args: Vec<String>,
    #[prost(bool, tag = "3")] term: bool,
    #[prost(uint32, optional, tag = "4")] grace_ms: Option<u32>,
}
#[derive(Clone, PartialEq, prost::Message)]
struct PPlanV1 {
    #[prost(message, repeated, tag = "1")] specs: Vec<PSpecV1>,
    #[prost(string, tag = "2")] note: String,
}
#[derive(Clone, PartialEq, prost::Message)]
struct PPlanV2 {
    #[prost(message, repeated, tag = "1")] specs: Vec<PSpecV2>,
    #[prost(string, tag = "2")] note: String,
}

fn verdict<T: PartialEq + std::fmt::Debug>(got: std::result::Result<T, String>, want: &T) -> String {
    match got {
        Ok(v) if &v == want => "ok".into(),
        Ok(v) => format!("WRONG ({v:?})"),
        Err(e) => format!("ERROR ({e})"),
    }
}

fn evolution() -> Result<()> {
    let s = |x: &str| x.to_string();
    let v1 = PlanV1 { specs: vec![SpecV1 { name: s("a"), args: vec![s("-x")] }, SpecV1 { name: s("b"), args: vec![] }], note: s("end") };
    let v2 = PlanV2 {
        specs: vec![
            SpecV2 { name: s("a"), args: vec![s("-x")], term: true, grace_ms: Some(500) },
            SpecV2 { name: s("b"), args: vec![], term: false, grace_ms: None },
        ],
        note: s("end"),
    };
    let v1_as_v2 = PlanV2 {
        specs: v1.specs.iter().map(|x| SpecV2 { name: x.name.clone(), args: x.args.clone(), term: false, grace_ms: None }).collect(),
        note: v1.note.clone(),
    };
    let v2_as_v1 = PlanV1 { specs: v2.specs.iter().map(|x| SpecV1 { name: x.name.clone(), args: x.args.clone() }).collect(), note: v2.note.clone() };

    println!("\nversion skew (old build reads new message / new build reads old message):");
    // minicbor
    let a = minicbor::decode::<PlanV1>(&minicbor::to_vec(&v2)?).map_err(|e| e.to_string());
    let b = minicbor::decode::<PlanV2>(&minicbor::to_vec(&v1)?).map_err(|e| e.to_string());
    println!("  minicbor  new→old fields: {}   old→new fields: {}", verdict(a, &v2_as_v1), verdict(b, &v1_as_v2));
    // postcard
    let a = postcard::from_bytes::<PlanV1>(&postcard::to_stdvec(&v2)?).map_err(|e| e.to_string());
    let b = postcard::from_bytes::<PlanV2>(&postcard::to_stdvec(&v1)?).map_err(|e| e.to_string());
    println!("  postcard  new→old fields: {}   old→new fields: {}", verdict(a, &v2_as_v1), verdict(b, &v1_as_v2));
    // prost
    let p2 = PPlanV2 {
        specs: vec![
            PSpecV2 { name: s("a"), args: vec![s("-x")], term: true, grace_ms: Some(500) },
            PSpecV2 { name: s("b"), args: vec![], term: false, grace_ms: None },
        ],
        note: s("end"),
    };
    let p1 = PPlanV1 { specs: vec![PSpecV1 { name: s("a"), args: vec![s("-x")] }, PSpecV1 { name: s("b"), args: vec![] }], note: s("end") };
    let a = PPlanV1::decode(p2.encode_to_vec().as_slice()).map_err(|e| e.to_string());
    let b = PPlanV2::decode(p1.encode_to_vec().as_slice()).map_err(|e| e.to_string());
    let p2_as_p1 = p1.clone();
    let p1_as_p2 = PPlanV2 {
        specs: vec![
            PSpecV2 { name: s("a"), args: vec![s("-x")], term: false, grace_ms: None },
            PSpecV2 { name: s("b"), args: vec![], term: false, grace_ms: None },
        ],
        note: s("end"),
    };
    println!("  prost     new→old fields: {}   old→new fields: {}", verdict(a, &p2_as_p1), verdict(b, &p1_as_p2));

    // New enum variant reaching an old reader, inside an Option.
    let l2 = ListenerV2 { name: s("api"), kind: Some(KindV2::Grpc) };
    let a = minicbor::decode::<ListenerV1>(&minicbor::to_vec(&l2)?).map_err(|e| e.to_string());
    println!("  minicbor  new enum variant (Option field) → old: {}", verdict(a, &ListenerV1 { name: s("api"), kind: None }));
    let a = postcard::from_bytes::<ListenerV1>(&postcard::to_stdvec(&l2)?).map_err(|e| e.to_string());
    println!("  postcard  new enum variant (Option field) → old: {}", verdict(a, &ListenerV1 { name: s("api"), kind: None }));
    let _ = msg::sample_hello();
    Ok(())
}
