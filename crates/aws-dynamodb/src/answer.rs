//! Turning the JSON a handler built into the operation's output, through the model.

use std::sync::OnceLock;

use aws_model_dynamodb::Operation;
use aws_model_dynamodb::tables::TABLES;
use aws_schema::patterns::Patterns;
use aws_wire::handler::reply_json;
use aws_wire::service::Reply;
use serde_json::Value;

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| Patterns::new(&TABLES))
}

/// The answer to `op` that says `json`.
pub(crate) fn reply(op: Operation, json: &Value) -> Reply {
    reply_json(&TABLES, patterns(), op.index(), json)
}
