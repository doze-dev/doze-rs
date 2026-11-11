#![no_main]
//! Any text, decoded as the input of a few operations of three models: no panic, no hang, and a reused arena stays usable.

use aws_schema::arena::Arena;
use aws_schema::json::decode;
use aws_schema::patterns::Patterns;
use aws_schema::tables::Tables;
use libfuzzer_sys::fuzz_target;

fn run(tables: &'static Tables, pick: u8, body: &str, arena: &mut Arena) {
    let patterns = Patterns::new(tables);
    let n = tables.n_ops();
    for step in 0..4 {
        let index = (u32::from(pick) + step * 37) % n.max(1);
        if let Some(shape) = tables.op(index).and_then(|op| op.input) {
            arena.clear();
            let _ = decode(tables, &patterns, shape, body, arena);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&pick, rest)) = data.split_first() else { return };
    let Ok(body) = std::str::from_utf8(rest) else { return };
    let mut arena = Arena::new();
    run(&aws_model_sqs::tables::TABLES, pick, body, &mut arena);
    run(&aws_model_dynamodb::tables::TABLES, pick, body, &mut arena);
    run(&aws_model_ssm::tables::TABLES, pick, body, &mut arena);
});
