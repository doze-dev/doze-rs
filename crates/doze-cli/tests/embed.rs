//! A program that embeds the CLI (the desktop app) hands a command line to `doze_cli::process::main` when
//! `doze_cli::is_command` says it is the CLI's. This test is such a program: its `main` does exactly that, and the test
//! runs itself as `<this program> __daemon` (the stack's daemon, as `ctl::start` runs it with `daemon_exe()` =
//! `current_exe`) and as `<this program> __fetch` (the daemon's download child). Real processes, so `harness = false`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout,
    clippy::print_stderr
)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();
    // The embedding, as the app does it.
    if let Some(first) = argv.get(1).and_then(|a| a.to_str())
        && first.starts_with("__")
        && doze_cli::is_command(first)
    {
        return ExitCode::from(doze_cli::process::main(argv));
    }
    // `cargo test` passes its own flags (`--nocapture`, a filter): this is the one test, run whatever they say.
    for w in doze_cli::HIDDEN_WORDS {
        assert!(doze_cli::is_command(w), "{w}");
    }
    #[cfg(unix)]
    {
        let dir = Dir(std::path::PathBuf::from(format!(
            "/tmp/dz-embed-{}",
            std::process::id()
        )));
        std::fs::create_dir_all(&dir.0).unwrap();
        fetch(&dir.0);
        daemon(&dir.0);
    }
    println!("embed: the embedding entry point runs __daemon and __fetch");
    ExitCode::SUCCESS
}

#[cfg(unix)]
struct Dir(std::path::PathBuf);

#[cfg(unix)]
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `__fetch`: the daemon's download child answers from a mirror on disk, through this program.
#[cfg(unix)]
fn fetch(dir: &std::path::Path) {
    use doze_fetch::child::{ChildDownloads, Request};
    let mirror = dir.join("mirror/valkey");
    std::fs::create_dir_all(&mirror).unwrap();
    let junk = b"not an archive";
    let sha = doze_fetch::registry::signed::sha256_hex(junk);
    std::fs::write(mirror.join("valkey-9.1.1.tar.gz"), junk).unwrap();
    let triple = format!(
        "{}-{}",
        std::env::consts::ARCH,
        if cfg!(target_os = "macos") {
            "apple-darwin"
        } else {
            "unknown-linux-gnu"
        }
    );
    std::fs::write(
        mirror.join("index.yaml"),
        format!(
            "engines:\n  valkey:\n    versions:\n      \"9\": 9.1.1\n    artifacts:\n      9.1.1:\n        {triple}:\n          url: valkey-9.1.1.tar.gz\n          sha256: {sha}\n"
        ),
    )
    .unwrap();
    let child = ChildDownloads {
        program: std::env::current_exe().unwrap(),
        args: vec![],
        env: vec![
            ("DOZE_HOME".into(), dir.join("home").display().to_string()),
            (
                "DOZE_MIRROR".into(),
                format!("file://{}", dir.join("mirror").display()),
            ),
            (
                "DOZE_REGISTRY".into(),
                dir.join("no-registry").display().to_string(),
            ),
        ],
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let r = rt
        .block_on(child.ask(&Request::ResolveMajor {
            engine: "valkey".into(),
            major: "9".into(),
        }))
        .unwrap();
    assert_eq!((r.version.as_str(), r.error), ("9.1.1", None), "__fetch");
}

/// `__daemon --config <doze.hcl>`: this program serves the stack on its socket until SIGTERM, as `doze` would.
#[cfg(unix)]
fn daemon(dir: &std::path::Path) {
    let home = dir.join("home");
    let config = dir.join("doze.hcl");
    std::fs::write(
        &config,
        format!(
            "stack = \"embed\"\nhome = \"{}\"\n\napp \"web\" {{\n  run = \"exec sleep 600\"\n}}\n",
            home.display()
        ),
    )
    .unwrap();
    let key = doze_projects::StackKey::for_config(&home, &config);
    std::fs::create_dir_all(key.dir()).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["__daemon", "--config"])
        .arg(&config)
        .env("DOZE_HOME", &home)
        .env("DOZE_REGISTRY", dir.join("no-registry"))
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let served = loop {
        if key.socket().exists() {
            break true;
        }
        if child.try_wait().unwrap().is_some() || std::time::Instant::now() > deadline {
            break false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    // Stop exactly the process this test started.
    let _ = doze_os::process::terminate(child.id());
    let status = child.wait().unwrap();
    assert!(served, "__daemon served no socket ({status})");
    assert!(status.success(), "__daemon stopped on SIGTERM: {status}");
}
