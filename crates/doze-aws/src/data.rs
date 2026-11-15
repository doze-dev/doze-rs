//! A person's data: what doze-aws keeps on disk, service by service and resource by resource, and the ways to remove it. One
//! engine answers both `doze-aws data` (in this process when no server runs, over `/_doze/data` when one does) and the console.
//!
//! Sizes come from two places and are reported as such: the disk (every file under a service's directory, so nothing a service
//! forgets to count goes unseen) and the services (`aws_wire::storage::Storage`: what each resource holds, and what removing it
//! gives back). Every removal ends in a compaction, so the space reaches the disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aws_wire::service::Service;
use aws_wire::storage::{Emptied, Prune, Resource, StorageError, Usage};
use serde_json::{Value, json};

use crate::services::Svc;

/// The engine: the data directory and every service that may keep something in it.
pub(crate) struct Data {
    dir: PathBuf,
    services: Vec<(&'static Svc, Arc<dyn Service>)>,
}

/// One service's part.
#[derive(Debug, Default)]
pub(crate) struct Part {
    pub(crate) name: &'static str,
    pub(crate) title: &'static str,
    /// Every file under its directory.
    pub(crate) disk: u64,
    pub(crate) usage: Usage,
    pub(crate) names: Option<aws_wire::storage::Names>,
    /// Why the service could not say what it holds, when it could not.
    pub(crate) error: Option<String>,
}

/// A service and its catalogue entry.
type Picked<'a> = (&'static Svc, &'a Arc<dyn Service>);

/// What `empty` was asked to do, by name.
#[derive(Clone, Debug, Default)]
pub(crate) struct Target {
    pub(crate) service: String,
    /// `None`: whichever region holds a resource of that name, when exactly one does.
    pub(crate) region: Option<String>,
    pub(crate) name: String,
    pub(crate) prune: Prune,
}

/// Why a removal was refused, with the status the HTTP hook answers.
#[derive(Debug)]
pub(crate) struct Refusal {
    pub(crate) status: u16,
    pub(crate) message: String,
}

fn refuse(status: u16, message: impl Into<String>) -> Refusal {
    Refusal {
        status,
        message: message.into(),
    }
}

impl Data {
    pub(crate) fn new(dir: PathBuf, services: Vec<(&'static Svc, Arc<dyn Service>)>) -> Self {
        Self { dir, services }
    }

    /// Every service with something on disk, or only `only`. Each one asked wakes (its regions with files only).
    pub(crate) async fn report(&self, only: Option<&str>) -> Result<Vec<Part>, Refusal> {
        let mut parts = Vec::new();
        for (svc, service) in self.pick(only)? {
            let dir = self.dir.join(dir_name(svc.name));
            let disk = dir_bytes(&dir);
            if disk == 0 && only.is_none() {
                continue;
            }
            let mut part = Part {
                name: svc.name,
                title: svc.title,
                disk,
                ..Part::default()
            };
            if let Some(storage) = service.storage() {
                part.names = Some(storage.names());
                match storage.usage().await {
                    Ok(usage) => part.usage = usage,
                    Err(e) => part.error = Some(e),
                }
            }
            parts.push(part);
        }
        Ok(parts)
    }

    /// The data directory's own size: every file in it, the services' and anything else.
    pub(crate) fn total(&self) -> u64 {
        dir_bytes(&self.dir)
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    /// Removes one resource's data (or the part `target.prune` names). `--unused` names no resource: it runs in every region the
    /// service has files in, unless one is named.
    pub(crate) async fn empty(&self, target: &Target) -> Result<Value, Refusal> {
        let (svc, service) = self
            .pick(Some(&target.service))?
            .into_iter()
            .next()
            .ok_or_else(|| refuse(404, "no such service"))?;
        let storage = service.storage().ok_or_else(|| {
            refuse(
                400,
                format!("{} keeps nothing that can be emptied", svc.title),
            )
        })?;
        let names = storage.names();
        let regions = match (&target.region, target.prune.unused) {
            (Some(r), _) => vec![r.clone()],
            (None, true) if target.name.is_empty() => {
                let usage = storage.usage().await.map_err(|e| refuse(500, e))?;
                usage.files.into_iter().map(|f| f.region).collect()
            }
            (None, _) => vec![self.region_of(svc, &target.name, storage).await?],
        };
        let mut done = Emptied::default();
        for region in &regions {
            let one = storage
                .empty(region, &target.name, &target.prune)
                .await
                .map_err(|e| match e {
                    StorageError::NotFound => refuse(
                        404,
                        format!(
                            "{} has no {} named {} in {region}",
                            svc.title, names.one, target.name
                        ),
                    ),
                    StorageError::Unsupported(why) => refuse(400, why),
                    StorageError::Failed(why) => refuse(500, why),
                })?;
            done.add(one);
        }
        let shown = regions.join(", ");
        Ok(json!({
            "region": shown, "title": svc.title, "one": names.one, "units": names.units,
            "items": done.items, "bytes": done.bytes, "freed": done.freed,
        }))
    }

    /// The one region that has a resource of this name, so a person need not say.
    async fn region_of(
        &self,
        svc: &Svc,
        name: &str,
        storage: &dyn aws_wire::storage::Storage,
    ) -> Result<String, Refusal> {
        let names = storage.names();
        // `unused` names no resource: the home region's files are where code lives unless said otherwise.
        let usage = storage.usage().await.map_err(|e| refuse(500, e))?;
        let mut regions: Vec<String> = usage
            .resources
            .iter()
            .filter(|r| r.name == name)
            .map(|r| r.region.clone())
            .collect();
        regions.dedup();
        match regions.as_slice() {
            [one] => Ok(one.clone()),
            [] => Err(refuse(
                404,
                format!(
                    "{} has no {} named {name}{}",
                    svc.title,
                    names.one,
                    suggest(&usage.resources, name)
                ),
            )),
            many => Err(refuse(
                409,
                format!(
                    "{name} is a {} in {} regions ({}): say which with --region",
                    names.one,
                    many.len(),
                    many.join(", ")
                ),
            )),
        }
    }

    /// Removes everything, or one service's everything.
    pub(crate) async fn reset(
        &self,
        only: Option<&str>,
        dry_run: bool,
    ) -> Result<Vec<(&'static str, Emptied)>, Refusal> {
        let mut out = Vec::new();
        for (svc, service) in self.pick(only)? {
            let Some(storage) = service.storage() else {
                continue;
            };
            let dir = self.dir.join(dir_name(svc.name));
            if !dir.exists() {
                continue;
            }
            let before = dir_bytes(&dir);
            let mut done = storage
                .reset(dry_run)
                .await
                .map_err(|e| refuse(500, format!("{}: {e}", svc.title)))?;
            if !dry_run {
                // What the disk got back, measured: the files a service keeps beside its database count too.
                done.freed = before.saturating_sub(dir_bytes(&dir));
            }
            out.push((svc.name, done));
        }
        Ok(out)
    }

    /// Compacts every service's files; answers the bytes freed.
    pub(crate) async fn compact(&self) -> Result<u64, Refusal> {
        let mut freed = 0;
        for (svc, service) in self.pick(None)? {
            if let Some(storage) = service.storage()
                && self.dir.join(dir_name(svc.name)).exists()
            {
                freed += storage
                    .compact()
                    .await
                    .map_err(|e| refuse(500, format!("{}: {e}", svc.title)))?;
            }
        }
        Ok(freed)
    }

    fn pick(&self, only: Option<&str>) -> Result<Vec<Picked<'_>>, Refusal> {
        let only = match only {
            Some(name) => Some(
                crate::services::find(name)
                    .ok_or_else(|| refuse(404, crate::services::unknown(name)))?
                    .name,
            ),
            None => None,
        };
        Ok(self
            .services
            .iter()
            .filter(|(svc, _)| only.is_none_or(|o| o == svc.name))
            .map(|(svc, s)| (*svc, s))
            .collect())
    }
}

/// Where a service keeps its files under the data directory.
pub(crate) fn dir_name(service: &str) -> &str {
    match service {
        "stepfunctions" => "states",
        other => other,
    }
}

/// Every file's size under `dir`, recursively; 0 when there is nothing.
pub(crate) fn dir_bytes(dir: &Path) -> u64 {
    let Ok(read) = std::fs::read_dir(dir) else {
        return 0;
    };
    read.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_bytes(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// ` (did you mean X?)` for the nearest name, when one is near.
fn suggest(resources: &[Resource], name: &str) -> String {
    resources
        .iter()
        .map(|r| (strsim::jaro_winkler(&r.name, name), &r.name))
        .filter(|(score, _)| *score > 0.8)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, n)| format!(" (did you mean {n}?)"))
        .unwrap_or_default()
}

/// A report as JSON: the answer of `GET /_doze/data`, read back by the command.
pub(crate) fn to_json(dir: &Path, total: u64, parts: &[Part]) -> Value {
    json!({
        "data_dir": dir.display().to_string(),
        "bytes": total,
        "services": parts.iter().map(part_json).collect::<Vec<_>>(),
    })
}

fn part_json(p: &Part) -> Value {
    json!({
        "name": p.name,
        "title": p.title,
        "bytes": p.disk,
        "one": p.names.map(|n| n.one),
        "many": p.names.map(|n| n.many),
        "units": p.names.map(|n| n.units),
        "prunes": p.names.map(|n| n.prunes.to_vec()),
        "error": p.error,
        "files": p.usage.files.iter().map(|f| json!({"region": f.region, "bytes": f.bytes, "reclaimable": f.reclaimable})).collect::<Vec<_>>(),
        "resources": p.usage.resources.iter().map(resource_json).collect::<Vec<_>>(),
    })
}

fn resource_json(r: &Resource) -> Value {
    json!({
        "region": r.region,
        "name": r.name,
        "bytes": r.bytes,
        "items": r.items,
        "notes": r.notes.iter().map(|n| json!({
            "bytes": n.bytes,
            "text": n.text,
            "prune": n.prune.as_ref().map(prune_json),
        })).collect::<Vec<_>>(),
    })
}

pub(crate) fn prune_json(p: &Prune) -> Value {
    json!({
        "before_ms": p.before_ms,
        "prefix": p.prefix,
        "old_versions": p.old_versions,
        "unused": p.unused,
        "dry_run": p.dry_run,
    })
}

pub(crate) fn prune_of(v: &Value) -> Prune {
    Prune {
        before_ms: v.get("before_ms").and_then(Value::as_i64),
        prefix: v.get("prefix").and_then(Value::as_str).map(str::to_owned),
        old_versions: v
            .get("old_versions")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        unused: v.get("unused").and_then(Value::as_bool).unwrap_or(false),
        dry_run: v.get("dry_run").and_then(Value::as_bool).unwrap_or(false),
    }
}

pub(crate) fn emptied_json(e: &Emptied) -> Value {
    json!({"items": e.items, "bytes": e.bytes, "freed": e.freed})
}

/// A number field of a JSON object, 0 when absent.
pub(crate) fn num(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// A text field of a JSON object, empty when absent.
pub(crate) fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// `v` with `key` set (when it is an object).
pub(crate) fn with(mut v: Value, key: &str, value: impl Into<Value>) -> Value {
    if let Some(o) = v.as_object_mut() {
        o.insert(key.to_owned(), value.into());
    }
    v
}

/// A reset's answer: what each service removed.
pub(crate) fn reset_json(done: &[(&'static str, Emptied)]) -> Value {
    let services: Vec<Value> = done
        .iter()
        .map(|(name, e)| with(emptied_json(e), "service", *name))
        .collect();
    json!({"services": services})
}
