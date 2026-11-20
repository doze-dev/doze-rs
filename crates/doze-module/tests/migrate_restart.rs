//! `doze migrate` on a running stack whose block changed since the daemon started, end to end with real processes: the
//! daemon is this test program run as `doze __daemon` (and its downloads as `doze __fetch`), the echo module comes from
//! a registry signed here, its engines from a mirror on disk. Nothing reaches the network.
//!
//! The stack runs echo engine 1 with `api` reading it; doze.hcl is then edited to engine 2 and `doze migrate e` asked:
//! the daemon is restarted (a new pid) so it reads doze.hcl as it is now, the module moves the data (its marker goes from
//! 1 to 2, the old major's build fetched for it), and `api`, awake before, wakes again only after the move. `doze down`
//! leaves no process.
//!
//! A test of its own (`harness = false`): the program must answer `__daemon` and `__fetch` as `doze` does.

#[cfg(unix)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some(doze_daemon::ctl::ARG) => {
            let rest: Vec<String> = args.get(2..).unwrap_or_default().to_vec();
            std::process::exit(
                match doze_daemon::entry::main(&rest, doze_cli::ctx::VERSION) {
                    std::process::ExitCode::SUCCESS => 0,
                    _ => 1,
                },
            );
        }
        Some(doze_cli::fetch_child::ARG) => std::process::exit(i32::from(fetch())),
        Some(test::INNER) => test::run(),
        // `cargo test` passes its own flags (`--nocapture`, a filter): this is the one test, run whatever they say.
        _ => test::outer(),
    }
}

#[cfg(not(unix))]
fn main() {
    // No local control socket for the daemon yet on Windows (core plan W).
}

/// The download child, with the test registry's key in place of the built-in one.
#[cfg(unix)]
fn fetch() -> u8 {
    let Ok(home) = doze_os::home::doze_home() else {
        return 1;
    };
    let registry = doze_cli::registry::Config {
        bases: doze_fetch::registry::bases_from_env(),
        key: test::key().verifying_key(),
        budget: std::time::Duration::from_secs(5),
    };
    doze_cli::fetch_child::serve(&home, registry)
}

#[cfg(unix)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout,
    clippy::indexing_slicing
)]
mod test {
    use base64ct::{Base64, Encoding};
    use ed25519_dalek::{Signer, SigningKey};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    pub(crate) fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn sign_hex(hex: &str) -> String {
        Base64::encode_string(&key().sign(hex.as_bytes()).to_bytes())
    }

    /// `files` (archive path, source) as a gzipped tar; its sha256.
    fn archive(dir: &Path, name: &str, files: &[(&str, &str)]) -> String {
        let stage = dir.join(format!("{name}.stage"));
        for (inside, from) in files {
            let to = stage.join(inside);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(from, &to).unwrap();
        }
        let out = dir.join(name);
        let ok = Command::new("tar")
            .arg("czf")
            .arg(&out)
            .arg("-C")
            .arg(&stage)
            .arg("bin")
            .status()
            .unwrap()
            .success();
        assert!(ok, "tar");
        std::fs::remove_dir_all(&stage).unwrap();
        doze_fetch::registry::signed::sha256_hex(&std::fs::read(&out).unwrap())
    }

    /// The registry (doze/echo, one release for this machine, signed with [`key`]) and the engine mirror (echo 1 and
    /// 2), under `root`.
    fn publish(root: &Path) {
        use doze_fetch::registry::index::{self, Artifact, Index, Release};
        let triple = doze_fetch::triple();
        let reg = root.join("registry/doze/echo");
        std::fs::create_dir_all(&reg).unwrap();
        let plugin = env!("CARGO_BIN_EXE_doze-module-echo");
        let file = format!("echo-plugin-0.1.0-{triple}.tar.gz");
        let sha = archive(&reg, &file, &[("bin/echo-plugin", plugin)]);
        let mut idx = Index {
            module: "echo".into(),
            namespace: "doze".into(),
            channels: Some([("stable".to_string(), "0.1.0".to_string())].into()),
            ..Index::default()
        };
        idx.releases.insert(
            "0.1.0".into(),
            Release {
                protocol: i64::from(doze_module_proto::version::CURRENT.major),
                engines: vec!["1".into(), "2".into()],
                artifacts: [(
                    triple.to_string(),
                    Artifact {
                        url: file,
                        sig: sign_hex(&sha),
                        sha256: sha,
                    },
                )]
                .into(),
                doze: String::new(),
            },
        );
        idx.signature = sign_hex(&doze_fetch::registry::signed::sha256_hex(
            index::payload(&idx).as_bytes(),
        ));
        let release = &idx.releases["0.1.0"];
        let art = &release.artifacts[&triple];
        let text = serde_json::json!({
            "schema": 1, "module": "echo", "namespace": "doze", "channels": idx.channels,
            "signature": idx.signature,
            "releases": {"0.1.0": {"protocol": release.protocol, "engines": release.engines,
                "artifacts": {triple.clone(): {"url": art.url, "sha256": art.sha256, "sig": art.sig}}}},
        });
        std::fs::write(reg.join("index.yaml"), text.to_string()).unwrap();

        let mirror = root.join("mirror/echo");
        std::fs::create_dir_all(&mirror).unwrap();
        let engine = env!("CARGO_BIN_EXE_doze-module-echo-engine");
        let mut artifacts = String::new();
        for full in ["1.0.0", "2.4.1"] {
            let name = format!("echo-{full}-{triple}.tar.gz");
            let sha = archive(&mirror, &name, &[("bin/doze-module-echo-engine", engine)]);
            artifacts.push_str(&format!(
                "      {full}:\n        {triple}:\n          url: {name}\n          sha256: {sha}\n"
            ));
        }
        std::fs::write(
            mirror.join("index.yaml"),
            format!(
                "engines:\n  echo:\n    versions:\n      \"1\": 1.0.0\n      \"2\": 2.4.1\n    artifacts:\n{artifacts}"
            ),
        )
        .unwrap();
    }

    /// The machine as the CLI sees it, the daemon's program being this one.
    struct Here;

    impl doze_cli::Host for Here {
        fn var(&self, name: &str) -> Option<String> {
            std::env::var(name).ok()
        }
        fn open_url(&self, _url: &str) -> std::io::Result<()> {
            Ok(())
        }
        fn spawn_background(&self, _args: &[&str]) -> std::io::Result<()> {
            Ok(())
        }
        fn exe(&self) -> Option<PathBuf> {
            std::env::current_exe().ok()
        }
    }

    /// `doze <args>` from `dir`: its code, stdout and stderr.
    fn doze(dir: &Path, args: &[&str]) -> (u8, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut ctx = doze_cli::Ctx {
            host: &Here,
            out: &mut out,
            err: &mut err,
            term: doze_cli::Terminals::default(),
            home: doze_os::home::doze_home().unwrap(),
            cwd: dir.to_path_buf(),
            registry: doze_cli::registry::Config {
                bases: doze_fetch::registry::bases_from_env(),
                key: key().verifying_key(),
                budget: std::time::Duration::from_secs(5),
            },
            now: std::time::SystemTime::now(),
        };
        let mut argv: Vec<std::ffi::OsString> = vec!["doze".into()];
        argv.extend(args.iter().map(Into::into));
        let code = doze_cli::run(argv, &mut ctx).code();
        let s = |b: Vec<u8>| String::from_utf8_lossy(&b).into_owned();
        (code, s(out), s(err))
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    /// Stops what the test started, by exact pid, however it ends.
    struct Reap(Vec<u32>);

    impl Drop for Reap {
        fn drop(&mut self) {
            for pid in self
                .0
                .iter()
                .copied()
                .filter(|p| doze_os::process::alive(*p))
            {
                let _ = doze_os::process::terminate(pid);
            }
        }
    }

    /// The argument the test runs itself with, its environment set (the daemon and the download children inherit it).
    pub(crate) const INNER: &str = "__migrate_restart";

    /// Publishes the registry and the mirror, then runs [`run`] in a process of its own with the environment pointing
    /// at them; its exit code is this one's.
    pub(crate) fn outer() {
        // A short root: the control socket's path must fit a Unix socket's.
        let root = PathBuf::from(format!("/tmp/dzmr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        publish(&root);
        let status = Command::new(std::env::current_exe().unwrap())
            .arg(INNER)
            .env("DZMR_ROOT", &root)
            .env("DOZE_HOME", root.join("home"))
            .env("DOZE_REGISTRY", root.join("registry"))
            .env(
                "DOZE_MIRROR",
                format!("file://{}", root.join("mirror").display()),
            )
            .env("DOZE_ZONE", "off")
            .status()
            .unwrap();
        let _ = std::fs::remove_dir_all(&root);
        std::process::exit(status.code().unwrap_or(1));
    }

    pub(crate) fn run() {
        let root = PathBuf::from(std::env::var_os("DZMR_ROOT").unwrap());
        let project = root.join("p");
        std::fs::create_dir_all(&project).unwrap();
        let port = free_port();
        let hcl = |v: &str| {
            format!(
                "stack = \"mig\"\n\nmodule echo \"e\" {{\n  version = \"{v}\"\n  port    = {port}\n}}\n\napp \"api\" {{\n  run = \"exec sleep 600\"\n  env = {{ E = module.e.url }}\n}}\n"
            )
        };
        std::fs::write(project.join("doze.hcl"), hcl("1")).unwrap();
        let key =
            doze_projects::StackKey::for_config(&root.join("home"), &project.join("doze.hcl"));
        let pid = || doze_projects::daemon_pid(&key).map(|p| p.pid);

        let (code, out, err) = doze(&project, &["up"]);
        let mut reap = Reap(pid().into_iter().collect());
        assert_eq!(code, 0, "up: {out}{err}");
        let first = pid().expect("a daemon after up");
        let marker = std::fs::read_dir(root.join("home/projects"))
            .unwrap()
            .flatten()
            .map(|e| e.path().join("clusters/e/ECHO_VERSION"))
            .find(|p| p.exists())
            .expect("e's data marker");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "1");

        // The edit: e's block now declares engine 2. The running daemon still has engine 1's block.
        std::fs::write(project.join("doze.hcl"), hcl("2")).unwrap();
        let (code, out, err) = doze(&project, &["migrate", "--dry-run", "e"]);
        assert_eq!(code, 0, "{out}{err}");
        assert!(out.contains("restart the daemon"), "{out}");
        let (code, out, err) = doze(&project, &["migrate", "e"]);
        reap.0.extend(pid());
        assert_eq!(code, 0, "migrate: {out}{err}");
        assert!(
            err.contains("restarted the doze daemon: e's block changed"),
            "{err}"
        );
        assert!(out.starts_with("✓ e migrated\n"), "{out}");
        assert!(out.contains("e is on echo 2 now"), "{out}");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "2");
        let second = pid().expect("a daemon after migrate");
        assert_ne!(first, second, "the daemon was restarted");
        assert!(!doze_os::process::alive(first), "the old daemon is gone");
        // api was awake before the restart: awake again, after the move.
        let (_, status, _) = doze(&project, &["--json", "status"]);
        let v: serde_json::Value = serde_json::from_str(&status).unwrap();
        let api = v["data"]["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "api")
            .unwrap()
            .clone();
        assert_eq!(api["state"], "active", "{api}");

        let (code, out, err) = doze(&project, &["down"]);
        assert_eq!(code, 0, "down: {out}{err}");
        assert!(!doze_os::process::alive(second), "down stopped the daemon");
        drop(reap);
        println!("migrate_restart: ok");
    }
}
