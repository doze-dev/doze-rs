//! A binary that links all 18 models, their tables and the decoder, and uses each, so that `ls -l` on its release build is the
//! cost of the model layer (PLAN.md, A2 exit: "a use-everything build ... has a stated size ceiling").
//!
//! It reads a JSON body on stdin and decodes it as the input of every operation of every service, printing how many accept it.

use std::io::Read;

use aws_modelcheck::services;
use aws_schema::arena::Arena;
use aws_schema::json::decode;
use aws_schema::patterns::Patterns;

fn main() {
    let mut body = String::new();
    if std::io::stdin().read_to_string(&mut body).is_err() {
        return;
    }
    let mut arena = Arena::new();
    let mut accepted = 0usize;
    for svc in services() {
        let patterns = Patterns::new(svc.tables);
        for i in 0..svc.tables.n_ops() {
            let Some(shape) = svc.tables.op(i).and_then(|op| op.input) else {
                continue;
            };
            arena.clear();
            if decode(svc.tables, &patterns, shape, &body, &mut arena).is_ok() {
                accepted += 1;
            }
        }
    }
    println!("{accepted}");
}
