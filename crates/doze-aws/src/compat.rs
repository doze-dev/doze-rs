//! `doze-aws compat` (hidden): every API this build answers and, for each operation in AWS's model of it, whether it is built.
//! Read from the services themselves (`Service::serves`), so the compatibility page (`tools/compat/generate.ts`) cannot claim
//! an operation the binary refuses, or miss one it serves. Nothing is opened: the services are made in a scratch directory and
//! never called.

use serde_json::{Value, json};

/// Runs the command; the exit code.
pub(crate) fn run(cfg: &crate::config::Config) -> i32 {
    let scratch = std::env::temp_dir().join(format!("doze-aws-compat-{}", std::process::id()));
    let mut given = crate::config::Given::new();
    given.insert(
        "data-dir",
        (
            vec![scratch.display().to_string()],
            crate::config::Source::Flag,
        ),
    );
    let cfg =
        crate::config::Config::load(&given, None, &std::env::current_dir().unwrap_or_default())
            .unwrap_or_else(|_| cfg.clone());
    let tasks = doze_runtime::Tasks::new();
    let all: Vec<&'static crate::services::Svc> = crate::services::ALL.iter().collect();
    let gw = aws_gateway::Config::new(std::net::SocketAddr::from(([127, 0, 0, 1], 0)));
    let (gateway, _resume, _data, _apis) = crate::wire::gateway(&cfg, gw, &all, &tasks);
    let apis: Vec<Value> = gateway
        .services()
        .iter()
        .map(|s| {
            let meta = s.meta();
            let tables = s.tables();
            let ops: Vec<Value> = (0..)
                .map_while(|i| tables.op(i).map(|op| (i, op.name)))
                .map(|(i, name)| json!({"name": name, "built": s.serves(i)}))
                .collect();
            json!({
                "sdk_id": meta.sdk_id,
                "signing_name": meta.signing_name,
                "shape_name": meta.shape_name,
                "ops": ops,
            })
        })
        .collect();
    drop(std::fs::remove_dir_all(&scratch));
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"version": crate::VERSION, "apis": apis}))
            .unwrap_or_default()
    );
    0
}
