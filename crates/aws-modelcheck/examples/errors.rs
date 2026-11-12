//! Prints a model's errors: `cargo run -p aws-modelcheck --example errors -- sqs`.

use aws_modelcheck::services;

fn main() {
    let want = std::env::args().nth(1).unwrap_or_default();
    for svc in services().into_iter().filter(|s| s.name == want) {
        let t = svc.tables;
        for i in 0..t.n_errs() {
            if let Some(e) = t.err(i) {
                let shape = t.shape(e.shape).map_or("?", |s| s.name);
                println!(
                    "{shape:<48} {:<48} {} {}",
                    e.code,
                    e.status,
                    if e.server { "server" } else { "client" }
                );
            }
        }
    }
}
