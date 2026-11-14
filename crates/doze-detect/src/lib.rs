//! What a project needs, read from its files (`doze init`, Go's `internal/detect`): the programs it runs, the
//! modules and services they talk to, the one-off commands that prepare them. It reads go.mod and the imports and
//! environment variables of Go main packages, package.json, a compose file, a Makefile and `.env`, and starts nothing.
//!
//! Every finding carries the reason it was found, which `doze init` writes above its block, so the person can see why
//! it is there and delete it when the guess is wrong (CLI proposal F6: never invented).
//!
//! **Modules are known only from what they declare.** Go kept a table of engines here (which Go imports mean
//! postgres, which variables mean redis). This crate has none: each module's own declaration ([`Declared`], from the
//! registry's `meta.yaml`) says which packages, images and variables mean it, its usual block name, port and version.
//! A module that declares nothing is never guessed. What is not a module (mongo, mailpit, nats, …) is a `service`
//! recipe in [`services`] (core plan O9).

mod files;
mod go;
mod node;
mod render;
pub mod services;

pub use files::{ComposeService, compose_services, make_targets, slug};
pub use go::requires;
pub use render::{download_block, quote, render_module, scaffold, service_block, wiring};

use doze_module_proto::msg::hints::Detect;
use std::collections::BTreeMap;
use std::path::Path;

/// What one module declares, as `doze init` and `doze add` use it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Declared {
    /// The module type (`valkey`).
    pub module: String,
    /// The name a new block of it gets (`cache`).
    pub label: String,
    /// Its usual port; 0 when it declares none.
    pub port: u16,
    /// The engine version written for a new block; empty writes none (lint then asks for it).
    pub version: String,
    /// What it is (`database`, `cache`): an app reading a database waits for the project's migration task.
    pub category: String,
    /// How a project shows it uses the module.
    pub detect: Detect,
}

/// What a folder suggests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// The stack's name, from the folder's.
    pub stack: String,
    pub apps: Vec<App>,
    pub modules: Vec<Module>,
    pub services: Vec<Service>,
    pub tasks: Vec<Task>,
    /// `.env` when one sits in the folder.
    pub env_file: Option<String>,
    /// Things worth saying that no block carries.
    pub notes: Vec<String>,
}

impl Found {
    /// Whether nothing at all was found.
    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
            && self.modules.is_empty()
            && self.services.is_empty()
            && self.tasks.is_empty()
    }
}

/// A program the project runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct App {
    pub name: String,
    pub run: String,
    /// 0 when none was found.
    pub port: u16,
    /// The code reads `$PORT`: it expects to be told where to listen.
    pub reads_port: bool,
    pub why: String,
    /// The environment variables its code reads that something found answers.
    pub uses: Vec<Use>,
}

/// One variable an app reads, and what answers it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Use {
    pub env: String,
    /// The module type or service name that answers it.
    pub kind: String,
    /// `url` or `address`.
    pub attr: String,
}

/// A module the project talks to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Module {
    pub module: String,
    /// The block's name.
    pub name: String,
    pub port: u16,
    pub version: String,
    pub why: String,
    /// The module declares itself a database.
    pub database: bool,
}

/// Something the project needs that is not a module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    /// From a compose file; empty when it came from code.
    pub image: String,
    pub port: u16,
    pub why: String,
}

/// A command that prepares something.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Task {
    pub name: String,
    pub run: String,
    /// Run only when asked (a seed).
    pub manual: bool,
    pub why: String,
}

/// A scan in progress: what the modules declare, and what has been found so far (each module and service once, with
/// the first reason it was found for).
struct Scan<'a> {
    declared: &'a [Declared],
    modules: BTreeMap<String, Module>,
    services: BTreeMap<String, Service>,
}

impl Scan<'_> {
    fn add_module(&mut self, d: &Declared, why: String) {
        self.modules
            .entry(d.module.clone())
            .or_insert_with(|| Module {
                module: d.module.clone(),
                name: if d.label.is_empty() {
                    d.module.clone()
                } else {
                    d.label.clone()
                },
                port: d.port,
                version: d.version.clone(),
                why,
                database: d.category == "database",
            });
    }

    fn add_service(&mut self, name: &str, port: u16, image: &str, why: String) {
        self.services
            .entry(name.to_owned())
            .or_insert_with(|| Service {
                name: name.to_owned(),
                image: image.to_owned(),
                port,
                why,
            });
    }

    /// Go packages (go.mod's requirements, or a main package's imports): each module or service whose declared
    /// prefixes one matches.
    fn note_go_packages(&mut self, packages: &[String], why: &str) {
        for p in packages {
            let declared = self.declared;
            for d in declared {
                if d.detect
                    .packages
                    .get("go")
                    .is_some_and(|ps| has_prefix(p, ps))
                {
                    self.add_module(d, format!("{why} {p}"));
                }
            }
            for r in services::RECIPES {
                if r.go.iter().any(|g| p.starts_with(g)) {
                    self.add_service(r.name, r.port, "", format!("{why} {p}"));
                }
            }
        }
    }

    /// The variables an app reads that something answers, noting the module or service when the variable is the
    /// only sign of it.
    fn uses(&mut self, envs: &[String]) -> Vec<Use> {
        let mut out = Vec::new();
        for e in envs {
            let declared = self.declared;
            for d in declared {
                if let Some(attr) = d.detect.env.get(e) {
                    self.add_module(d, format!("the code reads {e}"));
                    out.push(Use {
                        env: e.clone(),
                        kind: d.module.clone(),
                        attr: attr.clone(),
                    });
                }
            }
            for r in services::RECIPES {
                if r.envs.contains(&e.as_str()) {
                    self.add_service(r.name, r.port, "", format!("the code reads {e}"));
                    out.push(Use {
                        env: e.clone(),
                        kind: r.name.to_owned(),
                        attr: "address".into(),
                    });
                }
            }
        }
        out
    }
}

fn has_prefix(s: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|p| s.starts_with(p.as_str()))
}

/// Reads `dir` against what the modules in `declared` say they are recognised by. Modules come out in `declared`'s
/// order, services by name.
pub fn scan(dir: &Path, declared: &[Declared]) -> Found {
    let abs = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut found = Found {
        stack: slug(
            &abs.file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default(),
        ),
        ..Found::default()
    };
    let mut s = Scan {
        declared,
        modules: BTreeMap::new(),
        services: BTreeMap::new(),
    };
    go::scan(&mut s, dir, &abs, &mut found.apps);
    node::scan(&mut s, dir, &mut found.apps);
    files::compose(&mut s, dir);
    found.tasks = files::makefile(dir);
    files::env_files(dir, &mut found);
    found.modules = declared
        .iter()
        .filter_map(|d| s.modules.remove(&d.module))
        .collect();
    found.services = s.services.into_values().collect();
    found
}
