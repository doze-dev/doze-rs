use s0_module_proto::{msg, pb};
fn main() {
    let r = std::hint::black_box(pb::plan_req(&msg::sample_plan_req()));
    let p = std::hint::black_box(pb::spawn_plan(&msg::sample_spawn_plan()));
    println!("{} {}", r.inst.map(|i| i.name.len()).unwrap_or(0), p.specs.len());
}
