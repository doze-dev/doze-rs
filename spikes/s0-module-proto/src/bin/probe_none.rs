use s0_module_proto::msg::*;
fn main() {
    let r = std::hint::black_box(sample_plan_req());
    let p = std::hint::black_box(sample_spawn_plan());
    let h = std::hint::black_box(sample_hello());
    println!("{} {} {}", r.inst.name.len(), p.specs.len(), h.caps.len());
}
