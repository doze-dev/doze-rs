//! What a folder with no doze.hcl says it runs, for the empty folder's Found table (Go `internal/detect`, the part the
//! page shows): Go main packages and go.mod's requirements, package.json's dependencies and its dev script, a compose
//! file's images, a Makefile's preparing targets. It reads files and starts nothing.
//!
//! This is the table only: `doze init` (the CLI) does the full reading and writes the file, so what is written is the
//! CLI's word, never this guess. Source imports and the environment variables code reads, which Go also follows, are
//! left to the CLI.

use doze_module_proto::msg::hints::{Detect, ModuleHints};
use std::collections::BTreeMap;
use std::path::Path;

/// One thing found: its name, where it came from, and the block it becomes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub where_: String,
    pub becomes: String,
}

/// What a folder suggests, by kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detected {
    pub apps: Vec<Found>,
    pub modules: Vec<Found>,
    pub services: Vec<Found>,
    pub tasks: Vec<Found>,
}

impl Detected {
    /// Each kind found, counted: "2 apps", "1 module".
    pub fn counts(&self) -> Vec<String> {
        [
            (self.apps.len(), "app"),
            (self.modules.len(), "module"),
            (self.services.len(), "service"),
            (self.tasks.len(), "task"),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, w)| format!("{n} {w}{}", doze_display::plural(n as u64)))
        .collect()
    }

    /// Every line, apps first.
    pub fn items(&self) -> Vec<Found> {
        [&self.apps, &self.modules, &self.services, &self.tasks]
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }
}

/// What makes a project need a module is the module's own declaration (its manifest's `detect`, the module contract in
/// docs/port/modules/sdk.md): the scan is handed the union of what the modules this machine knows declare, and knows no
/// engine of its own. A module that declares nothing is never guessed.
fn go_needs(d: &Detect, path: &str) -> bool {
    d.packages
        .get("go")
        .is_some_and(|ps| ps.iter().any(|g| path.starts_with(g.as_str())))
}

fn npm_needs(d: &Detect, dep: &str) -> bool {
    d.packages
        .get("npm")
        .is_some_and(|ps| ps.iter().any(|n| n == dep))
}

fn image_needs(d: &Detect, base: &str) -> bool {
    d.images.iter().any(|i| i == base)
}

/// The Makefile targets that read as preparing something.
const TASK_TARGETS: &[&str] = &[
    "migrate",
    "migrate-up",
    "db-migrate",
    "migrations",
    "seed",
    "db-seed",
    "setup",
    "bootstrap",
    "generate",
    "codegen",
];

struct Scan<'h> {
    hints: &'h [ModuleHints],
    out: Detected,
    modules: BTreeMap<String, Found>,
    services: BTreeMap<String, Found>,
}

impl Scan<'_> {
    fn module(&mut self, h: &ModuleHints, why: String) {
        self.modules
            .entry(h.module.clone())
            .or_insert_with(|| Found {
                name: h.label.clone(),
                where_: why,
                becomes: format!("module {} {:?}", h.module, h.label),
            });
    }

    fn service(&mut self, name: &str, image: &str, why: String) {
        self.services
            .entry(name.to_owned())
            .or_insert_with(|| Found {
                name: name.to_owned(),
                where_: if image.is_empty() {
                    why
                } else {
                    format!("image {image} · {why}")
                },
                becomes: format!("service {name:?}"),
            });
    }

    /// Every module whose declaration `pick` accepts is needed, for `why`; whether any was.
    fn need(&mut self, pick: impl Fn(&Detect) -> bool, why: &str) -> bool {
        let hints = self.hints;
        let mut any = false;
        for h in hints {
            if h.detect.as_ref().is_some_and(&pick) {
                self.module(h, why.to_owned());
                any = true;
            }
        }
        any
    }
}

/// Lowercase, every run of other characters one dash.
fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// Reads `dir`, guessing modules from `hints` (the union of what the known modules declare).
pub fn detect(dir: &Path, hints: &[ModuleHints]) -> Detected {
    let mut s = Scan {
        hints,
        out: Detected::default(),
        modules: BTreeMap::new(),
        services: BTreeMap::new(),
    };
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok();
    if let Some(m) = read("go.mod") {
        go(&mut s, dir, &m);
    }
    if let Some(p) = read("package.json") {
        node(&mut s, dir, &p);
    }
    let compose = [
        "compose.yaml",
        "compose.yml",
        "docker-compose.yaml",
        "docker-compose.yml",
    ];
    if let Some((file, text)) = compose.iter().find_map(|f| read(f).map(|t| (*f, t))) {
        for (svc, image) in compose_images(&text) {
            let base = image_base(&image);
            let why = format!("{file} has a {base} service");
            if !s.need(|d| image_needs(d, &base), &why) {
                s.service(
                    &slug(&svc),
                    &image,
                    format!("{file} has a {svc} service ({image})"),
                );
            }
        }
    }
    if let Some(text) = ["Makefile", "makefile", "GNUmakefile"]
        .iter()
        .find_map(|f| read(f))
    {
        for t in make_targets(&text)
            .into_iter()
            .filter(|t| TASK_TARGETS.contains(&t.as_str()))
        {
            let name = slug(&t);
            s.out.tasks.push(Found {
                becomes: format!("task {name:?}"),
                where_: format!("make {t}"),
                name,
            });
        }
    }
    s.out.modules = s.modules.into_values().collect();
    s.out.services = s.services.into_values().collect();
    s.out
}

fn go(s: &mut Scan, dir: &Path, gomod: &str) {
    for line in gomod.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if let [path, _, ..] = f.as_slice()
            && !path.starts_with("//")
            && !["module", "go", "require", "toolchain"].contains(path)
            && path.contains('.')
        {
            let why = format!("the code imports {path}");
            s.need(|d| go_needs(d, path), &why);
        }
    }
    if go_main(&dir.join("main.go")) {
        let name = dir
            .file_name()
            .map(|n| slug(&n.to_string_lossy()))
            .unwrap_or_default();
        s.out.apps.push(app(&name, "go run .", ""));
    }
    let Ok(cmds) = std::fs::read_dir(dir.join("cmd")) else {
        return;
    };
    let mut mains: Vec<String> = cmds
        .flatten()
        .filter(|e| go_main(&e.path().join("main.go")))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    mains.sort();
    for m in mains {
        s.out
            .apps
            .push(app(&slug(&m), &format!("go run ./cmd/{m}"), ""));
    }
}

fn go_main(f: &Path) -> bool {
    std::fs::read_to_string(f).is_ok_and(|t| {
        t.lines()
            .any(|l| l.trim_start().starts_with("package main"))
    })
}

fn app(name: &str, run: &str, dir: &str) -> Found {
    let mut where_ = run.to_owned();
    if !dir.is_empty() {
        where_.push_str(&format!(" · in {dir}"));
    }
    Found {
        name: name.to_owned(),
        where_,
        becomes: format!("app {name:?}"),
    }
}

fn node(s: &mut Scan, dir: &Path, text: &str) {
    let Ok(pkg) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let deps: Vec<&str> = ["dependencies", "devDependencies"]
        .iter()
        .filter_map(|k| pkg.get(k).and_then(|v| v.as_object()))
        .flat_map(|o| o.keys().map(String::as_str))
        .collect();
    for d in &deps {
        let why = format!("package.json depends on {d}");
        s.need(|h| npm_needs(h, d), &why);
    }
    let scripts = pkg.get("scripts").and_then(|v| v.as_object());
    let Some(script) = ["dev", "start", "serve"]
        .into_iter()
        .find(|n| scripts.is_some_and(|o| o.contains_key(*n)))
    else {
        return;
    };
    let name = pkg
        .get("name")
        .and_then(|v| v.as_str())
        .map(slug)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "web".into());
    let pm = [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
    ]
    .into_iter()
    .find(|(f, _)| dir.join(f).is_file())
    .map_or("npm", |(_, p)| p);
    s.out
        .apps
        .push(app(&name, &format!("{pm} run {script}"), ""));
}

/// An image without its registry tag: `postgres:17` → `postgres`.
fn image_base(image: &str) -> String {
    let no_digest = image.split('@').next().unwrap_or_default();
    let last_slash = no_digest.rfind('/').map_or(0, |i| i + 1);
    match no_digest.get(last_slash..).and_then(|t| t.find(':')) {
        Some(c) => no_digest
            .get(..last_slash + c)
            .unwrap_or(no_digest)
            .to_owned(),
        None => no_digest.to_owned(),
    }
    .trim_start_matches("docker.io/")
    .trim_start_matches("library/")
    .to_owned()
}

// fuzz:parser fuzz_detect
/// A compose file's services and their images, read by indentation (the top-level `services:`, each service one
/// level in, its `image:` under it), sorted by service name.
pub fn compose_images(text: &str) -> Vec<(String, String)> {
    let mut out = BTreeMap::new();
    let (mut inside, mut level, mut current) = (false, None::<usize>, None::<String>);
    for raw in text.lines() {
        let line = raw.split(" #").next().unwrap_or_default().trim_end();
        let body = line.trim_start();
        if body.is_empty() || body.starts_with('#') {
            continue;
        }
        let indent = line.len() - body.len();
        if indent == 0 {
            inside = body == "services:";
            current = None;
            continue;
        }
        if !inside {
            continue;
        }
        let lvl = *level.get_or_insert(indent);
        if indent == lvl {
            current = body
                .strip_suffix(':')
                .map(|n| n.trim_matches(['"', '\'']).to_owned());
        } else if let (Some(svc), Some(img)) = (&current, body.strip_prefix("image:")) {
            out.insert(svc.clone(), img.trim().trim_matches(['"', '\'']).to_owned());
        }
    }
    out.into_iter().filter(|(_, i)| !i.is_empty()).collect()
}

// fuzz:parser fuzz_detect
/// A Makefile's rule names (`name:` at the start of a line, not `name:=`).
pub fn make_targets(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let (name, rest) = l.split_once(':')?;
            let ok = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            (ok && !rest.starts_with('=')).then(|| name.to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests;
