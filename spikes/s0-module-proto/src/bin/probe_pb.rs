use prost::Message;
use s0_module_proto::{msg, pb};
fn main() {
    let r = std::hint::black_box(pb::plan_req(&msg::sample_plan_req()));
    let p = std::hint::black_box(pb::spawn_plan(&msg::sample_spawn_plan()));
    let a = pb::PlanReq::decode(r.encode_to_vec().as_slice()).unwrap();
    let b = pb::SpawnPlan::decode(p.encode_to_vec().as_slice()).unwrap();
    println!("{} {}", a.inst.map(|i| i.name.len()).unwrap_or(0), b.specs.len());
}
