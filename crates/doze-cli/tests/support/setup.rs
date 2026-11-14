//! The `@` lines of a transcript: the sandbox a scenario runs in (a project, this machine's copies, the update
//! state, the environment).

use super::fixture::{self, FakeHost, Sandbox};

/// Builds the scenario's sandbox and host.
pub(crate) fn prepare(setup: &[String]) -> (Sandbox, FakeHost) {
    let sb = Sandbox::new("t");
    fixture::write_registry(&sb.registry());
    let mut host = FakeHost::default();
    for d in setup {
        let (verb, arg) = d.split_once(' ').unwrap_or((d, ""));
        match verb {
            "registry" | "tty" | "no-json" => {}
            "env" => {
                let (k, v) = arg.split_once('=').unwrap();
                host.env.insert(k.into(), v.into());
            }
            "browser-fails" => host.browser_fails = true,
            "project" => project(&sb, arg),
            "cached" => {
                for f in ["index.yaml", "meta.yaml"] {
                    let to = sb.home().join("registry/doze").join(arg).join(f);
                    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                    std::fs::copy(sb.registry().join("doze").join(arg).join(f), to).unwrap();
                }
            }
            "copy" | "copy-bad" => {
                let (m, v) = arg.split_once(' ').unwrap();
                let t = doze_cli::platform::triple();
                let dir = sb.home().join(format!("modules/doze/{m}/{v}-{t}"));
                std::fs::create_dir_all(&dir).unwrap();
                let sha = if verb == "copy" {
                    fixture::archive_sha(m, v)
                } else {
                    "1".repeat(64)
                };
                std::fs::write(dir.join(".doze-archive-sha"), format!("sha256:{sha}\n")).unwrap();
            }
            "latest" => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let state = serde_json::json!({"checked_at": now, "latest": {"version": arg, "news": ["valkey sleeps with a snapshot"], "security": false}});
                std::fs::write(sb.home().join("update.json"), state.to_string()).unwrap();
            }
            other => panic!("unknown setup line `@ {other}`"),
        }
    }
    (sb, host)
}

fn project(sb: &Sandbox, kind: &str) {
    let p = sb.project();
    std::fs::write(
        p.join("doze.hcl"),
        "valkey \"cache\" { version = 9 }\npostgres \"db\" { version = 17 }\n",
    )
    .unwrap();
    let lock = match kind {
        "pinned" => fixture::lock_text(false),
        "tampered" => fixture::lock_text(true),
        "conflict" => "doze: 0.2.1\nmodules:\n<<<<<<< HEAD\n    doze/valkey:\n        version: 0.2.1\n=======\n    doze/valkey:\n        version: 0.3.0\n>>>>>>> main\n".into(),
        "bare" => return,
        other => panic!("unknown project `{other}`"),
    };
    std::fs::write(p.join("doze.lock"), lock).unwrap();
}
