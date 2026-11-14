//! What the oracle corpus cannot reach (`--var`, `DOZE_VAR_`, `DOZE_ZONE`, what a module declares
//! about itself, a module's override), and every place doze-config differs from Go on purpose:
//!
//! 1. `count` is capped at [`doze_config::MAX_COUNT`]; Go has no cap, and a typo of a few digits
//!    builds blocks until memory runs out.
//! 2. Where Go's choice follows a map's order (which reference, which argument is reported first,
//!    which "did you mean" when two names are as near), doze-config takes source (or name) order,
//!    so a load always says the same.
//! 3. A block's fingerprint hashes the spec's own canonical text, not Go's JSON (fingerprints are
//!    only compared within one build).
//! 4. Route names: Go refuses a module listener whose route name is another block's doze name.
//!    That needs the module's listener declarations through `Modules`; until then it is not
//!    checked (the extra-listener port check is).
//! 5. A module's body is the body as written (what the oracle's generic driver reads); Go hands an
//!    out-of-process module the file's source to decode itself (`DecodeRemote`), so its words for
//!    a mistake in its own body are the module's. The Rust modules declare a schema instead.
//! 6. Go cannot override a module block in the oracle (its generic driver rejects the override's
//!    body); doze-config lays the override over a module like any other block.
//! 7. A `log_size` too large for 64 bits is refused as "not a size"; Go wraps it to a negative
//!    byte count (the oracle's `multi/log-size` is listed as known in `oracle.rs`).
//! 8. The shape pass gives a module's default port only if the module is in the cache
//!    (`Modules::cached`); Go took it from a table compiled into doze, cached or not. The table
//!    and its `temporal` exception are now each module's to declare.
//! 9. A sibling named on the command line as `./x.doze.hcl` is read once (Go read it twice and
//!    refused its own blocks as taken), and a folder whose name holds glob characters (`g[x]`)
//!    loads its files (Go's `Glob` read the name as a pattern).
//! 10. Deep chains (locals, `after`, references) are walked without recursion: Go's 20,000-local
//!     chain loads here too, in well under a second, where a recursive walk overflows the stack.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use doze_config::modules::{Call, Listener, ModuleInfo, Modules};
use doze_config::{Config, Error, Options, load, parse};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use support::{Fixtures, OracleHost};

struct Stack {
    dir: PathBuf,
    home: PathBuf,
}

impl Stack {
    fn new(name: &str, files: &[(&str, &str)]) -> Stack {
        let root = std::env::temp_dir().join(format!("doze-config-{}-{name}", std::process::id()));
        let dir = root.join("stack");
        let home = root.join("home");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        for (n, text) in files {
            std::fs::write(dir.join(n), text).unwrap();
        }
        Stack { dir, home }
    }

    fn host(&self, zone: Option<&str>) -> OracleHost {
        OracleHost {
            home: self.home.clone(),
            cwd: self.dir.clone(),
            zone: zone.map(str::to_owned),
        }
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        if let Some(root) = self.dir.parent() {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

fn opts<'a>(host: &'a OracleHost, modules: &'a dyn Modules) -> Options<'a> {
    Options {
        host,
        modules,
        vars: BTreeMap::new(),
        version: "dev".into(),
    }
}

fn load_one(name: &str, text: &str, modules: &dyn Modules) -> Result<Config, Error> {
    let s = Stack::new(name, &[("doze.hcl", text)]);
    let host = s.host(None);
    load("doze.hcl", &opts(&host, modules))
}

fn summary(e: &Error) -> String {
    e.diags
        .iter()
        .next()
        .map_or_else(|| e.text.clone(), |d| d.summary.clone())
}

#[test]
fn count_is_capped() {
    let e = load_one(
        "count",
        "app \"w\" {\n  count = 100000\n  run = \"x\"\n}\n",
        &Fixtures,
    )
    .unwrap_err();
    assert_eq!(summary(&e), "invalid count");
    assert_eq!(
        e.diags.iter().next().unwrap().detail,
        format!("must be at most {}", doze_config::MAX_COUNT)
    );
    let ok = load_one(
        "count-ok",
        "app \"w\" {\n  count = 3\n  run = \"x\"\n}\n",
        &Fixtures,
    )
    .unwrap();
    assert_eq!(ok.instances.len(), 3);
}

#[test]
fn the_first_wrong_reference_in_source_order_is_reported() {
    let src = "app \"a\" {\n  run = \"x\"\n  env = {\n    B = module.zeta.url\n    A = module.alpha.url\n  }\n}\n";
    for _ in 0..5 {
        let e = load_one("order", src, &Fixtures).unwrap_err();
        assert_eq!(summary(&e), "reference to undeclared module \"zeta\"");
    }
}

#[test]
fn a_fingerprint_follows_the_spec() {
    let s = Stack::new("fp", &[("doze.hcl", "app \"w\" {\n  run = \"x\"\n}\n")]);
    let host = s.host(None);
    let a = load("doze.hcl", &opts(&host, &Fixtures)).unwrap();
    let b = load("doze.hcl", &opts(&host, &Fixtures)).unwrap();
    std::fs::write(s.dir.join("doze.hcl"), "app \"w\" {\n  run = \"y\"\n}\n").unwrap();
    let c = load("doze.hcl", &opts(&host, &Fixtures)).unwrap();
    let print = |c: &Config| c.block_fingerprints()["w"].fingerprint.clone();
    assert_eq!(print(&a), print(&b));
    assert_ne!(print(&a), print(&c));
}

#[test]
fn a_module_takes_its_override() {
    let s = Stack::new(
        "ov-module",
        &[
            ("doze.hcl", "module postgres \"db\" {\n  version = 16\n}\n"),
            (
                "local.override.doze.hcl",
                "module postgres \"db\" {\n  port = 6543\n}\n",
            ),
        ],
    );
    let host = s.host(None);
    let c = load("doze.hcl", &opts(&host, &Fixtures)).unwrap();
    assert_eq!(c.instance("db").unwrap().port, 6543);
}

#[test]
fn variables_come_from_the_command_line_then_the_environment() {
    let src = "variable \"region\" {}\nvariable \"n\" {\n  type = number\n}\napp \"w\" {\n  run = \"x ${var.region} ${var.n}\"\n}\n";
    let s = Stack::new("vars", &[("doze.hcl", src)]);
    let host = s.host(None);
    let mut o = opts(&host, &Fixtures);
    o.vars = BTreeMap::from([("region".into(), "eu".into()), ("n".into(), "4".into())]);
    let c = load("doze.hcl", &o).unwrap();
    assert_eq!(c.instance("w").unwrap().command().unwrap().run, "x eu 4");
    o.vars.insert("regoin".into(), "x".into());
    let e = load("doze.hcl", &o).unwrap_err();
    assert_eq!(
        e.text,
        "--var regoin: this stack declares no variable \"regoin\"; did you mean region?"
    );
}

#[test]
fn names_switched_off_refuse_domains() {
    let src = "defaults {\n  names = true\n}\napp \"web\" {\n  run = \"x\"\n  port = 8080\n  domains = [\"dev.acme.com\"]\n}\n";
    let s = Stack::new("zone", &[("doze.hcl", src)]);
    let on = s.host(None);
    assert!(load("doze.hcl", &opts(&on, &Fixtures)).is_ok());
    let off = s.host(Some("off"));
    let e = load("doze.hcl", &opts(&off, &Fixtures)).unwrap_err();
    assert_eq!(
        e.text,
        "\"web\" declares domains, but names are switched off here (DOZE_ZONE); unset it to serve them"
    );
}

/// A module that declares more than the fixtures do.
#[derive(Default)]
struct Declares {
    supervised: bool,
    admin_port: u16,
    unsupported: bool,
    lookup_error: bool,
}

impl Modules for Declares {
    fn lookup(&self, ty: &str, v: &str) -> Option<ModuleInfo> {
        if self.lookup_error {
            return None;
        }
        let _ = v;
        (ty == "box").then_some(ModuleInfo {
            versionless: true,
            default_port: 7000,
            supervised: self.supervised,
        })
    }
    fn cached(&self, ty: &str) -> Option<ModuleInfo> {
        self.lookup(ty, "")
    }
    fn lookup_error(&self, ty: &str) -> Option<String> {
        self.lookup_error
            .then(|| format!("the registry has no {ty} for this machine"))
    }
    fn names(&self) -> Vec<String> {
        vec!["box".into()]
    }
    fn check_support(&self, _ty: &str, version: &str) -> Result<(), String> {
        if self.unsupported {
            Err(format!(
                "box {version} is not served; run doze modules upgrade"
            ))
        } else {
            Ok(())
        }
    }
    fn listeners(&self, _call: &Call<'_>, _port: u16) -> Vec<Listener> {
        if self.admin_port == 0 {
            Vec::new()
        } else {
            vec![Listener {
                name: "admin".into(),
                port: self.admin_port,
            }]
        }
    }
}

#[test]
fn a_module_says_what_it_needs() {
    let m = Declares {
        admin_port: 8080,
        ..Declares::default()
    };
    let src = "module box \"b\" {}\napp \"w\" {\n  run = \"x\"\n  port = 8080\n}\n";
    let e = load_one("listener", src, &m).unwrap_err();
    assert!(
        e.text.starts_with(
            "port conflict: \"b\" (its admin listener) and \"w\" both use 127.0.0.1:8080"
        ),
        "{}",
        e.text
    );

    let m = Declares {
        supervised: true,
        ..Declares::default()
    };
    let e = load_one(
        "supervised",
        "module box \"b\" {\n  sleep = \"5m\"\n}\n",
        &m,
    )
    .unwrap_err();
    assert_eq!(
        summary(&e),
        "module box \"b\" can't sleep: doze doesn't sit in front of it, so nothing would wake it"
    );

    let m = Declares {
        unsupported: true,
        ..Declares::default()
    };
    let e = load_one("support", "module box \"b\" {\n  version = 3\n}\n", &m).unwrap_err();
    assert_eq!(summary(&e), "box 3 is not served; run doze modules upgrade");

    let m = Declares {
        lookup_error: true,
        ..Declares::default()
    };
    let e = load_one("lookup", "module box \"b\" {}\n", &m).unwrap_err();
    assert_eq!(summary(&e), "the registry has no box for this machine");

    let c = load_one(
        "default-port",
        "module box \"b\" {}\n",
        &Declares::default(),
    )
    .unwrap();
    assert_eq!(c.instance("b").unwrap().port, 7000);
}

#[test]
fn route_names_are_not_checked_yet() {
    // Go refuses a listener routed under another block's name; that needs listener routes from the
    // module (difference 4). Two blocks whose names collide are still refused.
    let src = "defaults {\n  names = true\n}\napp \"orders_pg\" {\n  run = \"x\"\n}\napp \"orders-pg\" {\n  run = \"y\"\n}\n";
    let e = load_one("routes", src, &Fixtures).unwrap_err();
    assert!(e.text.starts_with("domain conflict:"), "{}", e.text);
}

#[test]
fn the_shape_pass_takes_a_default_port_from_the_cache_only() {
    /// A catalog with postgres known but nothing cached.
    struct NotCached;
    impl Modules for NotCached {
        fn lookup(&self, ty: &str, v: &str) -> Option<ModuleInfo> {
            Fixtures.lookup(ty, v)
        }
        fn cached(&self, _ty: &str) -> Option<ModuleInfo> {
            None
        }
        fn names(&self) -> Vec<String> {
            Fixtures.names()
        }
    }
    let src = "module postgres \"db\" {\n  version = 16\n}\n";
    let s = Stack::new("shape-port", &[("doze.hcl", src)]);
    let host = s.host(None);
    let shape = doze_config::load_shape("doze.hcl", &opts(&host, &NotCached)).unwrap();
    assert_eq!(shape.instance("db").unwrap().port, 0);
    let cached = doze_config::load_shape("doze.hcl", &opts(&host, &Fixtures)).unwrap();
    assert_eq!(cached.instance("db").unwrap().port, 5432);
}

#[test]
fn deep_chains_load() {
    let n = 20_000;
    let mut src: String = (0..n)
        .map(|i| format!("  l{i} = local.l{}\n", i + 1))
        .collect();
    src = format!("locals {{\n{src}  l{n} = \"end\"\n}}\nstack = local.l0\n");
    let c = load_one("deep-locals", &src, &Fixtures).unwrap();
    assert_eq!(c.stack_name, "end");
    let tasks: String = (0..5_000)
        .map(|i| {
            format!(
                "task \"t{i}\" {{\n  run = \"x\"\n  manual = true\n  after = [task.t{}]\n}}\n",
                i + 1
            )
        })
        .collect();
    let src = format!("{tasks}task \"t5000\" {{\n  run = \"x\"\n  manual = true\n}}\n");
    let c = load_one("deep-tasks", &src, &Fixtures).unwrap();
    assert_eq!(c.tasks.len(), 5_001);
}

#[test]
fn a_source_parses_on_its_own() {
    let host = OracleHost {
        home: PathBuf::from("/nonexistent-home"),
        cwd: PathBuf::from("/"),
        zone: None,
    };
    let o = opts(&host, &Fixtures);
    let c = parse("app \"w\" {\n  run = \"x\"\n}\n", "doze.hcl", &o, false).unwrap();
    assert_eq!(c.instances.len(), 1);
    let e = parse("app \"w\" {", "doze.hcl", &o, false).unwrap_err();
    assert!(e.is_invalid());
    assert!(Path::new(&c.home).ends_with(".doze"));
}
