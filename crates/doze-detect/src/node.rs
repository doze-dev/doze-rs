//! Node: package.json's dependencies (each module or service whose declared npm packages it names), and its `dev`,
//! `start` or `serve` script as an app, with the variables the project's JavaScript and TypeScript read.

use crate::files::{exists, read_dir_sorted, slug};
use crate::go::{GETENV, captures};
use crate::{App, Scan, services};
use std::collections::BTreeSet;
use std::path::Path;

/// How deep, and how much, of the project's own source is read for variables.
const DEPTH: usize = 4;
const BUDGET: usize = 4 << 20;

pub(crate) fn scan(s: &mut Scan<'_>, dir: &Path, apps: &mut Vec<App>) {
    let Some(pkg) = std::fs::read(dir.join("package.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .filter(serde_json::Value::is_object)
    else {
        return;
    };
    let deps: BTreeSet<&str> = ["dependencies", "devDependencies"]
        .iter()
        .filter_map(|k| pkg.get(k).and_then(|v| v.as_object()))
        .flat_map(|m| m.keys().map(String::as_str))
        .collect();
    let declared = s.declared;
    for d in declared {
        let wanted = d
            .detect
            .packages
            .get("npm")
            .map(Vec::as_slice)
            .unwrap_or_default();
        if let Some(n) = wanted.iter().find(|n| deps.contains(n.as_str())) {
            s.add_module(d, format!("package.json depends on {n}"));
        }
    }
    for r in services::RECIPES {
        if let Some(n) = r.npm.iter().find(|n| deps.contains(*n)) {
            s.add_service(r.name, r.port, "", format!("package.json depends on {n}"));
        }
    }
    let scripts = pkg.get("scripts").and_then(|v| v.as_object());
    let Some(script) = ["dev", "start", "serve"]
        .into_iter()
        .find(|k| scripts.is_some_and(|m| m.contains_key(*k)))
    else {
        return;
    };
    let name = slug(pkg.get("name").and_then(|v| v.as_str()).unwrap_or_default());
    let mut text = String::new();
    read_tree(dir, 0, &mut text);
    let envs = captures(&GETENV, &text);
    apps.push(App {
        name: if name.is_empty() { "web".into() } else { name },
        run: format!("{} run {script}", package_manager(dir)),
        why: format!("package.json has a {script} script"),
        uses: s.uses(&envs),
        ..App::default()
    });
}

/// The project's own `.js`, `.ts` and `.mjs` files, to a bounded depth and size, without dependencies, builds or
/// hidden folders.
fn read_tree(dir: &Path, depth: usize, out: &mut String) {
    if depth > DEPTH || out.len() > BUDGET {
        return;
    }
    for (name, path) in read_dir_sorted(dir) {
        if path.is_dir() {
            if !matches!(name.as_str(), "node_modules" | "dist" | "build") && !name.starts_with('.')
            {
                read_tree(&path, depth + 1, out);
            }
            continue;
        }
        if [".js", ".ts", ".mjs"].iter().any(|x| name.ends_with(x))
            && let Ok(text) = std::fs::read_to_string(&path)
            && text.len() < 1 << 20
        {
            out.push_str(&text);
            out.push('\n');
        }
    }
}

fn package_manager(dir: &Path) -> &'static str {
    if exists(dir, "bun.lockb") || exists(dir, "bun.lock") {
        "bun"
    } else if exists(dir, "pnpm-lock.yaml") {
        "pnpm"
    } else if exists(dir, "yarn.lock") {
        "yarn"
    } else {
        "npm"
    }
}
