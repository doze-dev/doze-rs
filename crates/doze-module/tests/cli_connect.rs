//! `doze connect` end to end with the echo module: the CLI asks the daemon (in this process, the echo module's build
//! found by the test finder) for the instance, wakes it, finds the client in the engine's own build (no PATH at all),
//! and runs it with the module's arguments; a line typed into the client comes back through doze's front. Nothing in
//! the CLI names the module.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod daemon_support;

use daemon_support::*;
use doze_daemon::catalog::Catalog;
use doze_daemon::{Daemon, StackInfo};
use doze_engine::modules::Pinned;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Runs the client for real, with a line on its stdin, and keeps what it printed.
#[derive(Default)]
struct Real {
    ran: RefCell<Vec<(PathBuf, Vec<String>, Vec<(String, String)>, String)>>,
}

impl doze_cli::Host for Real {
    // No PATH: the client must come from the engine's own build.
    fn var(&self, _name: &str) -> Option<String> {
        None
    }
    fn open_url(&self, _url: &str) -> std::io::Result<()> {
        Ok(())
    }
    fn spawn_background(&self, _args: &[&str]) -> std::io::Result<()> {
        Ok(())
    }
    fn exe(&self) -> Option<PathBuf> {
        None
    }
    fn run_env(
        &self,
        program: &Path,
        args: &[String],
        env: &[(String, String)],
    ) -> std::io::Result<u8> {
        let mut child = std::process::Command::new(program)
            .args(args)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        child.stdin.take().unwrap().write_all(b"hello\n")?;
        let out = child.wait_with_output()?;
        self.ran.borrow_mut().push((
            program.to_path_buf(),
            args.to_vec(),
            env.to_vec(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        ));
        Ok(out.status.code().and_then(|c| u8::try_from(c).ok()).unwrap_or(1))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connect_wakes_the_instance_and_runs_the_modules_own_client() {
    let (dir, port) = project("connect");
    // A short home: the control socket's path must fit a Unix socket's.
    let home = PathBuf::from(format!("/tmp/dzc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let sock = doze_projects::StackKey::for_config(&home, &dir.join("doze.hcl")).socket();
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let lock = dir.join("doze.lock");
    let cat = Arc::new(Catalog::new(
        tokio::runtime::Handle::current(),
        Arc::new(Find::default()),
        Arc::new(Pinned {
            inner: Shared(Arc::new(Fetch::default())),
            pins: doze_daemon::catalog::lock_pins(&lock),
        }),
        lock,
        dir.join("home"),
    ));
    let env = BTreeMap::from([(
        "DOZE_HOME".to_string(),
        dir.join("home").to_string_lossy().into_owned(),
    )]);
    let a = doze_daemon::entry::assemble(&dir.join("doze.hcl"), "0.0.0", cat, env)
        .await
        .unwrap();
    // The front on the instance's port, as the daemon's own start opens it: the client connects there.
    let front = doze_daemon::front::Front::open(&a.engine, &a.fronted()).await;
    let d = Daemon::at(a.engine, a.labels, StackInfo::default(), sock).with_modules(a.modules);
    let s = d.serve().unwrap();

    let ran = tokio::task::spawn_blocking({
        let (dir, home) = (dir.clone(), home.clone());
        move || {
            let host = Real::default();
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let mut ctx = doze_cli::Ctx {
                host: &host,
                out: &mut out,
                err: &mut err,
                term: doze_cli::Terminals::default(),
                home,
                cwd: dir,
                registry: doze_cli::registry::Config {
                    bases: Vec::new(),
                    key: doze_cli::registry::signed::builtin_key().unwrap(),
                    budget: std::time::Duration::from_secs(1),
                },
                now: std::time::SystemTime::now(),
            };
            let argv = ["doze", "connect", "e"].map(Into::into).to_vec();
            let code = doze_cli::run(argv, &mut ctx).code();
            let said = String::from_utf8_lossy(&err).into_owned();
            (code, said, host.ran.into_inner())
        }
    })
    .await
    .unwrap();
    s.stop();
    front.close();
    d.engine().shutdown().await;
    let _ = std::fs::remove_dir_all(&home);

    let (code, said, ran) = ran;
    assert_eq!(code, 0, "{said}");
    assert!(said.contains("→ e waking…"), "{said}");
    assert!(
        said.contains(&format!("doze-module-echo-engine on e (127.0.0.1:{port})")),
        "{said}"
    );
    let (program, args, env, printed) = &ran[0];
    assert_eq!(
        program.file_name().unwrap(),
        "doze-module-echo-engine",
        "the engine's own build"
    );
    assert_eq!(args, &["client", "127.0.0.1", &port.to_string()]);
    assert!(env.is_empty(), "no password, no variable: {env:?}");
    assert_eq!(printed, "hello\n", "the line came back through doze's front");
}
