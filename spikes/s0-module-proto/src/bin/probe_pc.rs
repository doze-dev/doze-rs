use s0_module_proto::msg::*;
fn main() {
    let r = std::hint::black_box(sample_plan_req());
    let p = std::hint::black_box(sample_spawn_plan());
    let h = std::hint::black_box(sample_hello());
    let a: PlanReq = postcard::from_bytes(&postcard::to_stdvec(&r).unwrap()).unwrap();
    let b: SpawnPlan = postcard::from_bytes(&postcard::to_stdvec(&p).unwrap()).unwrap();
    let c: Hello = postcard::from_bytes(&postcard::to_stdvec(&h).unwrap()).unwrap();
    println!("{} {} {}", a.inst.name.len(), b.specs.len(), c.caps.len());
}
