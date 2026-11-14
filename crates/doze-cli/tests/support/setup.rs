//! The `@` lines of a transcript: the sandbox a scenario runs in (a project, this machine's copies, the update
//! state, the environment).

use super::fixture::{self, FakeHost, Sandbox};
use doze_control::fixture::{FixtureDaemon, Script};
use doze_projects::StackKey;
use std::path::Path;

/// Builds the scenario's sandbox and host.
pub(crate) fn prepare(setup: &[String]) -> (Sandbox, FakeHost, Option<FixtureDaemon>) {
    let sb = Sandbox::new("t");
    fixture::write_registry(&sb.registry());
    // The machine's files under the sandbox, and the zone off: what the machine's setup checks differs by system, so
    // transcripts (one text for every system) leave it out; tests/privileged.rs covers it, system by system.
    let mut host = FakeHost {
        system: Some(sb.root.join("system")),
        ..FakeHost::default()
    };
    host.env.insert("DOZE_ZONE".into(), "off".into());
    let mut daemon = None;
    for d in setup {
        let (verb, arg) = d.split_once(' ').unwrap_or((d, ""));
        match verb {
            "registry" | "tty" | "no-json" => {}
            "env" => {
                let (k, v) = arg.split_once('=').unwrap();
                let v = v.replace("{sandbox}", &sb.root.display().to_string());
                host.env.insert(k.into(), v);
            }
            "browser-fails" => host.browser_fails = true,
            "now" => host.now = Some(stamp(arg)),
            "daemon" => daemon = Some(start_daemon(&sb, arg)),
            // `@ logfile api r000001.s000.log <line>`: a line in the stack's log store.
            "logfile" => {
                let mut parts = arg.splitn(3, ' ');
                let (inst, seg, text) = (
                    parts.next().unwrap(),
                    parts.next().unwrap(),
                    parts.next().unwrap(),
                );
                append(&state_dir(&sb).join("logs").join(inst).join(seg), text);
            }
            // `@ journal <json>`: an event in the stack's journal.
            "journal" => append(&state_dir(&sb).join("run/journal.jsonl"), arg),
            "file" => {
                let (path, text) = arg.split_once(' ').unwrap();
                let to = sb.root.join(path);
                std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                std::fs::write(to, text.replace("\\n", "\n")).unwrap();
            }
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
    (sb, host, daemon)
}

/// A fixture daemon playing `tests/fixtures/daemon/<name>.ndjson` on the sandbox project's control socket.
fn start_daemon(sb: &Sandbox, name: &str) -> FixtureDaemon {
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/daemon/{name}.ndjson"));
    let script = Script::load(&script).unwrap();
    let sock = StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl")).socket();
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    FixtureDaemon::start(&sock, script).unwrap()
}

/// Whether this machine has the daemon's local control socket (Windows has none yet: doze-os `local`). The tests
/// that need a running daemon are skipped where it has none, as doze-control's own are.
pub(crate) fn sockets() -> bool {
    static ONCE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ONCE.get_or_init(probe)
}

fn probe() -> bool {
    let p = std::env::temp_dir().join(format!("dz-probe-{}.sock", std::process::id()));
    match doze_os::local::LocalListener::bind(&p) {
        Ok(_) => {
            let _ = std::fs::remove_file(&p);
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
            eprintln!("skipped: {e}");
            false
        }
        Err(e) => panic!("{e}"),
    }
}

/// The sandbox project's state folder under the doze home.
fn state_dir(sb: &Sandbox) -> std::path::PathBuf {
    StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"))
        .dir()
        .to_path_buf()
}

fn append(path: &Path, line: &str) {
    use std::io::Write;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(f, "{line}").unwrap();
}

fn stamp(s: &str) -> std::time::SystemTime {
    doze_control::Timestamp::parse(s)
        .and_then(|t| t.to_system_time())
        .unwrap()
}

fn project(sb: &Sandbox, kind: &str) {
    let p = sb.project();
    let hcl = if kind == "shop" {
        SHOP
    } else {
        "module valkey \"cache\" { version = 9 }\nmodule postgres \"db\" { version = 17 }\n"
    };
    std::fs::write(p.join("doze.hcl"), hcl).unwrap();
    let lock = match kind {
        "pinned" => fixture::lock_text(false),
        "tampered" => fixture::lock_text(true),
        "conflict" => "doze: 0.2.1\nmodules:\n<<<<<<< HEAD\n    doze/valkey:\n        version: 0.2.1\n=======\n    doze/valkey:\n        version: 0.3.0\n>>>>>>> main\n".into(),
        "bare" | "shop" => return,
        other => panic!("unknown project `{other}`"),
    };
    std::fs::write(p.join("doze.lock"), lock).unwrap();
}

/// The stack of Go's status goldens (cmd/doze/golden_test.go `shopConfig`), as written there.
const SHOP: &str = r#"
stack = "shop"

module postgres "db" {
  version = 16
  port    = 5432
}

module valkey "cache" {
  version = 9
  port    = 6379
}

module kafka "events" {
  version = 4
  port    = 9092
}

app "api" {
  run  = "go run ./cmd/api"
  port = 8080
  env  = { DATABASE_URL = module.db.url }
}

app "worker" {
  run = "go run ./cmd/worker"
  env = { KAFKA = module.events.address }
}

app "web" {
  run  = "go run ./cmd/web"
  port = 8090
}
"#;
