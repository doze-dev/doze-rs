//! A service's and a task's command built from what their blocks say (the second half of Go's
//! `decodeService` and `decodeTask`, `bootEdges` and `parseDownload`).

use crate::command::{command_config, resolve_dir};
use crate::gohcl::Decoded;
use crate::model::{
    COMMAND_ENGINE, Command, Condition, Config, Dependency, Instance, Kind, Ref, Spec as RunSpec,
    TaskDecl,
};
use doze_hcl::Value;
use doze_hcl::value::quote::go_quote;
use std::collections::BTreeMap;

const PLATFORMS: [&str; 4] = ["mac", "mac_intel", "linux", "linux_arm"];

pub(crate) fn task(
    cfg: &mut Config,
    raw: &Decoded<'_>,
    base: &str,
    name: &str,
    (after, refs): (&[Ref], &[Ref]),
) -> std::result::Result<Instance, String> {
    let env_file = raw.str("env_file");
    let decl = TaskDecl {
        name: name.to_owned(),
        run: raw.str("run"),
        dir: resolve_dir(base, &raw.str("dir")),
        env: raw.map("env"),
        env_file: if env_file.is_empty() {
            String::new()
        } else {
            resolve_dir(base, &env_file)
        },
        inputs: raw.list("inputs"),
        manual: raw.bool("manual"),
        after: after.to_vec(),
        refs: refs.to_vec(),
    };
    let c = Command {
        run: decl.run.clone(),
        dir: decl.dir.clone(),
        env: decl.env.clone(),
        env_file: decl.env_file.clone(),
        task: true,
        inputs: decl.inputs.clone(),
        manual: decl.manual,
        ..Command::default()
    };
    if let Some(t) = cfg.tasks.iter_mut().find(|t| t.name == name) {
        *t = decl;
    }
    let c = command_config(c, &crate::model::Restart::default(), name, 0)?;
    Ok(command_instance(Kind::Task, name, 0, c))
}

pub(crate) fn command_instance(kind: Kind, name: &str, port: u16, c: Command) -> Instance {
    Instance {
        kind: Some(kind),
        ty: COMMAND_ENGINE.into(),
        name: name.into(),
        version: "builtin".into(),
        port,
        spec: RunSpec::Command(Box::new(c)),
        ..Instance::default()
    }
}

/// Go's `bootEdges`: the body's references, then `after`, each once, with `after`'s waits.
pub(crate) fn boot_edges(
    me: &str,
    refs: &[Ref],
    after: &[Ref],
    waits: &BTreeMap<String, Condition>,
) -> Vec<Dependency> {
    let mut deps: Vec<Dependency> = Vec::new();
    for r in refs.iter().chain(after) {
        if r.name != me && !deps.iter().any(|d| d.name == r.name) {
            deps.push(Dependency {
                name: r.name.clone(),
                condition: waits.get(&r.name).copied().unwrap_or_default(),
            });
        }
    }
    deps
}

/// Go's `parseDownload`: a URL, or a map of platform to URL.
pub(crate) fn parse_download(v: &Value) -> std::result::Result<BTreeMap<String, String>, String> {
    if v.is_null() {
        return Ok(BTreeMap::new());
    }
    let bad = || {
        format!(
            "download takes a URL, or a map of platform ({}) to URL",
            PLATFORMS.join(", ")
        )
    };
    let check = |u: &str| {
        if u.starts_with("https://") || u.starts_with("http://") {
            Ok(())
        } else {
            Err(format!(
                "download = {}: want an http:// or https:// URL",
                go_quote(u)
            ))
        }
    };
    match v {
        Value::String(u) => {
            check(u)?;
            Ok(BTreeMap::from([(String::new(), u.clone())]))
        }
        Value::Object(m) | Value::Map(_, m) => {
            let mut out = BTreeMap::new();
            for (k, e) in m {
                let Value::String(u) = e else {
                    return Err(bad());
                };
                if !PLATFORMS.contains(&k.as_str()) {
                    return Err(format!(
                        "download: {} is not a platform (want {})",
                        go_quote(k),
                        PLATFORMS.join(", ")
                    ));
                }
                check(u)?;
                out.insert(k.clone(), u.clone());
            }
            if out.is_empty() { Err(bad()) } else { Ok(out) }
        }
        _ => Err(bad()),
    }
}
