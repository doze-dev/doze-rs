//! What the tests run doze in: a sandbox (a temporary doze home, a project directory, a registry on disk), never
//! `~/.doze` and never the network; a registry signed with the throwaway key of the registry's conformance fixture;
//! and a host that records what doze asked it to start instead of starting it. Each test file uses part of it.
#![allow(dead_code)]

use base64ct::Encoding as _;
use doze_cli::registry::index::{self, Artifact, Index, Release};
use doze_cli::registry::latest::{self, Latest};
use doze_cli::registry::{self, Base, signed};
use doze_cli::{Ctx, Host, Terminals};
use ed25519_dalek::{Signer, SigningKey};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime};

/// The conformance fixture's private key (PKCS#8 DER, base64): its last 32 bytes are the seed. A throwaway that has
/// never signed a release (doze-registry scripts/testdata/conformance/fixture.json says so).
const TEST_KEY_PKCS8: &str = "MC4CAQAwBQYDK2VwBCIEILMabEeImicKD0vMfy+ULv2puMlyeXVu8IsuG1BrWEdc";

/// Every target CI builds on, so the fixture has an artifact for whichever runs the test.
pub(crate) const TRIPLES: &[&str] = &[
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
    "aarch64-pc-windows-msvc",
    "x86_64-pc-windows-msvc",
];

pub(crate) fn test_key() -> SigningKey {
    use base64ct::{Base64, Encoding};
    let der = Base64::decode_vec(TEST_KEY_PKCS8).unwrap();
    let seed: [u8; 32] = der[der.len() - 32..].try_into().unwrap();
    SigningKey::from_bytes(&seed)
}

fn sign_hex(key: &SigningKey, hex: &str) -> String {
    use base64ct::{Base64, Encoding};
    Base64::encode_string(&key.sign(hex.as_bytes()).to_bytes())
}

/// The sha256 the fixture says a module's archive has: the same for every platform, so a transcript reads the same
/// on each.
pub(crate) fn archive_sha(module: &str, version: &str) -> String {
    signed::sha256_hex(format!("{module}-{version}").as_bytes())
}

/// A temporary directory tree, removed when dropped.
pub(crate) struct Sandbox {
    pub root: PathBuf,
}

impl Sandbox {
    pub(crate) fn new(tag: &str) -> Sandbox {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        // A short root: a project's control socket lies deep under it, and a Unix socket path has about 100 bytes.
        let base = Path::new("/tmp");
        let base = if base.is_dir() {
            base.to_path_buf()
        } else {
            std::env::temp_dir()
        };
        let root = base.join(format!("dz-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["home", "project", "registry"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let root = root.canonicalize().unwrap();
        Sandbox { root }
    }
    pub(crate) fn home(&self) -> PathBuf {
        self.root.join("home")
    }
    pub(crate) fn project(&self) -> PathBuf {
        self.root.join("project")
    }
    pub(crate) fn registry(&self) -> PathBuf {
        self.root.join("registry")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A module's fixture index: its releases (version, protocol, oldest doze), stable at the last, all signed.
pub(crate) fn index_for(module: &str, releases: &[(&str, i64, &str)]) -> Index {
    let key = test_key();
    let mut idx = Index {
        module: module.into(),
        namespace: "doze".into(),
        releases: BTreeMap::new(),
        channels: Some(BTreeMap::from([(
            "stable".into(),
            releases.last().unwrap().0.into(),
        )])),
        signature: String::new(),
    };
    for (v, protocol, doze) in releases {
        let artifacts = TRIPLES
            .iter()
            .map(|t| {
                let sha = archive_sha(module, v);
                let url = format!("https://example.invalid/{module}-{v}-{t}.tar.gz");
                (
                    t.to_string(),
                    Artifact {
                        sig: sign_hex(&key, &sha),
                        sha256: sha,
                        url,
                    },
                )
            })
            .collect();
        let engines = vec!["8".to_string(), "9".to_string()];
        idx.releases.insert(
            v.to_string(),
            Release {
                protocol: *protocol,
                engines,
                artifacts,
                doze: doze.to_string(),
            },
        );
    }
    idx.signature = sign_hex(&key, &signed::sha256_hex(index::payload(&idx).as_bytes()));
    idx
}

/// The index as the registry serves it (JSON is YAML; the parser reads both).
pub(crate) fn index_text(idx: &Index) -> String {
    let releases: serde_json::Map<String, serde_json::Value> = idx
        .releases
        .iter()
        .map(|(v, r)| {
            let arts: serde_json::Map<String, serde_json::Value> = r
                .artifacts
                .iter()
                .map(|(t, a)| (t.clone(), serde_json::json!({"url": a.url, "sha256": a.sha256, "sig": a.sig})))
                .collect();
            let mut rel = serde_json::json!({"protocol": r.protocol, "engines": r.engines, "artifacts": arts});
            if !r.doze.is_empty() {
                rel["doze"] = r.doze.clone().into();
            }
            (v.clone(), rel)
        })
        .collect();
    let v = serde_json::json!({
        "schema": 1, "module": idx.module, "namespace": idx.namespace,
        "releases": releases, "channels": idx.channels, "signature": idx.signature,
    });
    serde_json::to_string_pretty(&v).unwrap()
}

const VALKEY_META: &str = "title: Valkey\ntagline: In-memory cache, Redis-compatible.\ncategory: cache\nengine: valkey\nport: 6379\nexample: |-\n    valkey \"cache\" {\n      version   = 9\n      maxmemory = \"256mb\"\n    }\ndescription: A local Valkey server, the open-source Redis fork. Any redis client works.\nconfig:\n    arguments:\n        - name: version\n          type: number\n          desc: Engine major to run, 8 or 9.\n          required: true\n        - name: maxmemory_policy\n          type: string\n          default: noeviction\n          desc: Eviction policy.\n        - name: snapshot_on_sleep\n          type: bool\n          default: \"false\"\n          desc: Save before sleeping.\n          since: 0.3.0\n";

const POSTGRES_META: &str = "title: PostgreSQL\ntagline: The relational database.\ncategory: database\nengine: postgres\nport: 5432\nexample: postgres \"db\" { version = 17 }\ndescription: A real Postgres per service.\nconfig:\n    arguments:\n        - name: version\n          type: number\n          required: true\n    blocks:\n        - name: database\n          label: name\n          desc: A database to create.\n          arguments:\n            - name: owner\n              type: string\n";

/// Writes the fixture registry: valkey (0.2.1 on protocol 1, 0.3.0 on protocol 2), postgres 0.2.5, the catalog, the
/// namespace key and a release file for doze 9.9.9.
pub(crate) fn write_registry(dir: &Path) {
    let key = test_key();
    let valkey = index_for("valkey", &[("0.2.1", 1, ""), ("0.3.0", 2, "0.3.0")]);
    let postgres = index_for("postgres", &[("0.2.5", 2, "")]);
    write(&dir.join("doze/valkey/index.yaml"), &index_text(&valkey));
    write(&dir.join("doze/valkey/meta.yaml"), VALKEY_META);
    write(
        &dir.join("doze/postgres/index.yaml"),
        &index_text(&postgres),
    );
    write(&dir.join("doze/postgres/meta.yaml"), POSTGRES_META);
    let public = base64ct::Base64::encode_string(key.verifying_key().as_bytes());
    write(
        &dir.join("doze/keys.json"),
        &format!("{{\"namespace\":\"doze\",\"key\":\"{public}\"}}"),
    );
    write(
        &dir.join("index.json"),
        r#"{"namespaces":{"doze":{"modules":{"valkey":{},"postgres":{}}}}}"#,
    );
    let l = Latest {
        version: "9.9.9".into(),
        news: vec!["valkey sleeps with a snapshot".into()],
        security: false,
    };
    let sig = sign_hex(&key, &signed::sha256_hex(latest::payload(&l).as_bytes()));
    write(
        &dir.join(latest::PATH),
        &format!(
            "schema: 1\nversion: 9.9.9\nnews:\n  - valkey sleeps with a snapshot\nsecurity: false\nsignature: {sig}\n"
        ),
    );
}

/// The project's lock pinning valkey 0.2.1 and postgres 0.2.5 with the fixture's hashes (`tamper` changes valkey's
/// for this platform, so the mismatch named is the one that matters here, on every platform).
pub(crate) fn lock_text(tamper: bool) -> String {
    let mut out = String::from("doze: 0.2.1\nengines: {}\nmodules:\n");
    for (m, v, p) in [("postgres", "0.2.5", 2), ("valkey", "0.2.1", 1)] {
        out.push_str(&format!("    doze/{m}:\n        version: {v}\n        protocol: {p}\n        engines:\n            - \"8\"\n            - \"9\"\n        hashes:\n"));
        for t in TRIPLES {
            let sha = if tamper && m == "valkey" && *t == doze_cli::platform::triple() {
                "0".repeat(64)
            } else {
                archive_sha(m, v)
            };
            out.push_str(&format!("            {t}: sha256:{sha}\n"));
        }
    }
    out
}

/// A host for in-process runs: the environment is a map, and nothing is started.
#[derive(Default)]
pub(crate) struct FakeHost {
    pub env: HashMap<String, String>,
    pub spawned: RefCell<Vec<Vec<String>>>,
    pub opened: RefCell<Vec<String>>,
    pub browser_fails: bool,
    /// A clock that stands still (`@ now`).
    pub now: Option<SystemTime>,
    /// What `watch_interrupt` hands out, so a test can press Ctrl-C.
    pub interrupt: doze_os::interrupt::Interrupt,
}

impl Host for FakeHost {
    fn var(&self, name: &str) -> Option<String> {
        self.env.get(name).cloned()
    }
    fn open_url(&self, url: &str) -> std::io::Result<()> {
        self.opened.borrow_mut().push(url.to_string());
        if self.browser_fails {
            Err(std::io::Error::other("no browser here"))
        } else {
            Ok(())
        }
    }
    fn spawn_background(&self, args: &[&str]) -> std::io::Result<()> {
        self.spawned
            .borrow_mut()
            .push(args.iter().map(|a| a.to_string()).collect());
        Ok(())
    }
    fn exe(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/opt/homebrew/Cellar/doze/0.2.1/bin/doze"))
    }
    fn now(&self) -> SystemTime {
        self.now.unwrap_or_else(SystemTime::now)
    }
    fn watch_interrupt(&self) -> doze_os::interrupt::Interrupt {
        self.interrupt.clone()
    }
}

/// What one in-process run printed and how it ended.
pub(crate) struct Ran {
    pub code: u8,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `args` in-process in the sandbox with the fixture registry (or none answering when `online` is false).
pub(crate) fn run(
    sb: &Sandbox,
    host: &FakeHost,
    term: Terminals,
    online: bool,
    args: &[&str],
) -> Ran {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let bases = if online {
        vec![Base::Dir(sb.registry())]
    } else {
        vec![Base::Dir(sb.root.join("unreachable"))]
    };
    let key = signed::parse_key(&base64ct::Base64::encode_string(
        test_key().verifying_key().as_bytes(),
    ))
    .unwrap();
    let mut ctx = Ctx {
        host,
        out: &mut out,
        err: &mut err,
        term,
        home: sb.home(),
        cwd: sb.project(),
        registry: registry::Config {
            bases,
            key,
            budget: Duration::from_secs(3),
        },
        now: host.now(),
    };
    let argv = std::iter::once("doze")
        .chain(args.iter().copied())
        .map(Into::into)
        .collect();
    let code = doze_cli::run(argv, &mut ctx).code();
    Ran {
        code,
        stdout: String::from_utf8_lossy(&out).into_owned(),
        stderr: String::from_utf8_lossy(&err).into_owned(),
    }
}
