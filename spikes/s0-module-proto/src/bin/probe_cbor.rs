use s0_module_proto::msg::*;
fn main() {
    let r = std::hint::black_box(sample_plan_req());
    let p = std::hint::black_box(sample_spawn_plan());
    let h = std::hint::black_box(sample_hello());
    let a: PlanReq = minicbor::decode(&minicbor::to_vec(&r).unwrap()).unwrap();
    let b: SpawnPlan = minicbor::decode(&minicbor::to_vec(&p).unwrap()).unwrap();
    let c: Hello = minicbor::decode(&minicbor::to_vec(&h).unwrap()).unwrap();
    println!("{} {} {}", a.inst.name.len(), b.specs.len(), c.caps.len());
}
