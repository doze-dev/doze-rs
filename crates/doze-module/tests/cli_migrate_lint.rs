//! `doze migrate` and `doze lint` end to end with the echo module: the CLI (in this process) against the stack's
//! daemon (in this process too, the echo module's builds found by the test finder). migrate is the module's own move,
//! asked through the daemon's `migrate` call and said as the module said it; lint's module pass is the daemon's `lint`
//! op on doze.hcl as it is on disk, so a block the module refuses is exit 5 in the module's words. Nothing in the CLI
//! names the module.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod daemon_support;

use daemon_support::*;
use std::path::{Path, PathBuf};

/// A host that runs nothing: the CLI only asks the daemon here.
struct Quiet;

impl doze_cli::Host for Quiet {
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
}

/// `doze <args>` in-process from the project at `dir` with the doze home `home`: its code, stdout and stderr.
fn doze(dir: &Path, home: &Path, args: &[&str]) -> (u8, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut ctx = doze_cli::Ctx {
        host: &Quiet,
        out: &mut out,
        err: &mut err,
        term: doze_cli::Terminals::default(),
        home: home.to_path_buf(),
        cwd: dir.to_path_buf(),
        registry: doze_cli::registry::Config {
            bases: Vec::new(),
            key: doze_cli::registry::signed::builtin_key().unwrap(),
            budget: std::time::Duration::from_secs(1),
        },
        now: std::time::SystemTime::now(),
    };
    let mut argv: Vec<std::ffi::OsString> = vec!["doze".into()];
    argv.extend(args.iter().map(Into::into));
    let code = doze_cli::run(argv, &mut ctx).code();
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn migrate_and_lint_go_through_the_stacks_daemon_to_the_module() {
    let (dir, _port) = project("climig");
    // A short home: the control socket's path must fit a Unix socket's.
    let home = PathBuf::from(format!("/tmp/dzm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let sock = doze_projects::StackKey::for_config(&home, &dir.join("doze.hcl")).socket();
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let Some((d, s, _sock, front)) = serve_at(&dir, sock).await else {
        return;
    };
    let run = |args: &'static [&'static str]| {
        let (dir, home) = (dir.clone(), home.clone());
        tokio::task::spawn_blocking(move || doze(&dir, &home, args))
    };

    // The module's own refusal, in its words.
    let (code, _, err) = run(&["migrate", "e"]).await.unwrap();
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("e has no data yet: there is nothing to move"),
        "{err}"
    );
    // A task is not a module: said from doze.hcl, nothing asked.
    let (code, _, err) = run(&["migrate", "show"]).await.unwrap();
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("show is a task; doze migrate moves a module's data"),
        "{err}"
    );

    // Data on the old major: the module moves it, and what it said is printed.
    d.engine()
        .wake("e", doze_engine::Cause::Command)
        .await
        .unwrap();
    let marker = d.engine().stack().cluster_dir("e").join("ECHO_VERSION");
    std::fs::write(&marker, "1").unwrap();
    let (code, out, err) = run(&["migrate", "e"]).await.unwrap();
    assert_eq!(code, 0, "{out}{err}");
    assert!(err.contains("→ e migrating…"), "{err}");
    assert_eq!(
        out,
        "✓ e migrated\n  e is on echo 2 now (its echo 2.4.1 data was read)\n"
    );
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "2");

    // lint: the module pass through the daemon passes the file as it is.
    let (code, out, err) = run(&["lint"]).await.unwrap();
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.starts_with("✓ doze.hcl is valid: 2 modules"), "{out}");
    // An edit to e's block, written after the daemon started: still linted through the module, which answers its
    // variables for the edited block too (the task reads one).
    let hcl = std::fs::read_to_string(dir.join("doze.hcl")).unwrap();
    std::fs::write(
        dir.join("doze.hcl"),
        hcl.replace("greeting = \"hi\"", "greeting = \"hello\""),
    )
    .unwrap();
    let (code, out, err) = run(&["lint"]).await.unwrap();
    assert_eq!(code, 0, "{out}{err}");
    // A mistake the module pass finds: exit 5, in its words.
    std::fs::write(
        dir.join("doze.hcl"),
        hcl.replace("module.e.env.ECHO_URL", "module.e.env.NOSUCH"),
    )
    .unwrap();
    let (code, out, err) = run(&["lint"]).await.unwrap();
    assert_eq!(code, 5, "{out}{err}");
    assert!(
        err.contains("error[DZ0301]") && err.contains("NOSUCH"),
        "{err}"
    );

    s.stop();
    front.close();
    d.engine().shutdown().await;
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&dir);
}
