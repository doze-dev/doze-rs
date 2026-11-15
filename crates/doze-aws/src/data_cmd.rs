//! `doze-aws data`: see what is kept, and remove it. Talks to the running doze-aws over `/_doze/data` when one holds the data
//! directory; otherwise opens the services in this process (holding the directory's lock, so a server cannot start meanwhile) and
//! asks the same engine. Either way the answer is the same document, printed by `data_show`.
//!
//! Every removal is looked at first (a dry run), said in full, and asked about: `y` for one resource or service, the word `reset`
//! for everything. `--yes` skips the question; without a terminal and without `--yes` nothing is removed.

use std::io::{BufRead, IsTerminal, Write};
use std::sync::Arc;
use std::time::Duration;

use aws_wire::storage::Prune;
use serde_json::{Value, json};

use crate::cli::{DataAction, DataArgs, EmptyArgs, ResetArgs};
use crate::config::{Config, Source, display_path};
use crate::data::{self, Data, Target};
use crate::data_show::{count, size};
use crate::style::{Stream, Style};

/// Where the engine is.
enum Via {
    Server(crate::client::Client, Option<String>),
    Local(Local),
}

/// The engine in this process, its runtime, and the lock that keeps a server from starting under it.
struct Local {
    runtime: tokio::runtime::Runtime,
    data: Arc<Data>,
    tasks: doze_runtime::Tasks,
    _lock: doze_runtime::DataDirLock,
}

impl Via {
    fn open(cfg: &Config) -> Result<Self, String> {
        if let Some(found) = crate::locate::find(cfg) {
            let key = (found.auth == "verify").then(|| found.keys().1);
            return Ok(Self::Server(found.client()?, key));
        }
        let dir = cfg.data_dir();
        let lock = doze_runtime::DataDirLock::take(&dir, "doze-aws data").map_err(|e| match e {
            doze_runtime::LockError::Held { .. } => crate::startfail::data_dir_held(&dir),
            other => format!("doze-aws cannot use its data directory: {other}"),
        })?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let tasks = doze_runtime::Tasks::new();
        let all: Vec<&'static crate::services::Svc> = crate::services::ALL.iter().collect();
        let gw = aws_gateway::Config::new(std::net::SocketAddr::from(([127, 0, 0, 1], 0)));
        let (_gateway, _resume, data) = crate::wire::gateway(cfg, gw, &all, &tasks);
        Ok(Self::Local(Local {
            runtime,
            data,
            tasks,
            _lock: lock,
        }))
    }

    /// `GET /_doze/data`, or the engine's report.
    fn report(&self, only: Option<&str>) -> Result<Value, String> {
        match self {
            Self::Server(client, key) => {
                let path = only.map_or_else(
                    || "/_doze/data".to_owned(),
                    |s| format!("/_doze/data?service={s}"),
                );
                answer(client.hook(
                    &path,
                    None,
                    &headers(key.as_ref()),
                    Duration::from_secs(120),
                ))
            }
            Self::Local(l) => l.runtime.block_on(async {
                let parts = l.data.report(only).await.map_err(|r| r.message)?;
                Ok(data::to_json(l.data.dir(), l.data.total(), &parts))
            }),
        }
    }

    /// A removal: `POST /_doze/data/<what>` with `body`, or the engine's own.
    fn post(&self, what: &str, body: &Value) -> Result<Value, String> {
        match self {
            Self::Server(client, key) => {
                let bytes = serde_json::to_vec(body).map_err(|e| e.to_string())?;
                let path = format!("/_doze/data/{what}");
                answer(client.hook(
                    &path,
                    Some(&bytes),
                    &headers(key.as_ref()),
                    Duration::from_secs(600),
                ))
            }
            Self::Local(l) => l.runtime.block_on(local_post(&l.data, what, body)),
        }
    }

    fn close(self) {
        if let Self::Local(l) = self {
            // The services flush and close their files before the lock is let go.
            l.runtime.block_on(l.tasks.shutdown());
        }
    }
}

fn headers(key: Option<&String>) -> Vec<(&str, &str)> {
    key.map(|k| vec![("X-Doze-Key", k.as_str())])
        .unwrap_or_default()
}

fn answer(reply: Result<crate::client::Reply, String>) -> Result<Value, String> {
    let reply = reply?;
    let body: Value = serde_json::from_slice(&reply.body).unwrap_or(Value::Null);
    if reply.status == 200 {
        Ok(body)
    } else {
        Err(body.get("error").and_then(Value::as_str).map_or_else(
            || format!("doze-aws answered {}", reply.status),
            str::to_owned,
        ))
    }
}

async fn local_post(data: &Data, what: &str, body: &Value) -> Result<Value, String> {
    match what {
        "empty" => {
            let target = target_of(body);
            data.empty(&target).await.map_err(|r| r.message)
        }
        "reset" => {
            let only = body.get("service").and_then(Value::as_str);
            let dry = body
                .get("dry_run")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let done = data.reset(only, dry).await.map_err(|r| r.message)?;
            Ok(data::reset_json(&done))
        }
        _ => {
            let freed = data.compact().await.map_err(|r| r.message)?;
            Ok(json!({"freed": freed}))
        }
    }
}

fn target_of(body: &Value) -> Target {
    let text = |k: &str| body.get(k).and_then(Value::as_str).map(str::to_owned);
    Target {
        service: text("service").unwrap_or_default(),
        region: text("region"),
        name: text("name").unwrap_or_default(),
        prune: data::prune_of(body.get("prune").unwrap_or(&Value::Null)),
    }
}

/// Runs the command; the exit code.
pub(crate) fn run(args: DataArgs, cfg: &Config) -> i32 {
    let st = Style::of(Stream::Out);
    let via = match Via::open(cfg) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("doze-aws data: {e}");
            return 1;
        }
    };
    let result = match args.action {
        None => show(&via, &args, cfg, st),
        Some(DataAction::Empty(e)) => empty(&via, &e, cfg, st),
        Some(DataAction::Reset(r)) => reset(&via, &r, cfg, st),
        Some(DataAction::Compact) => via.post("compact", &json!({})).map(|v| {
            let freed = v.get("freed").and_then(Value::as_u64).unwrap_or(0);
            println!("Compacted: the disk got back {}.", size(freed));
        }),
    };
    via.close();
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("doze-aws data: {e}");
            1
        }
    }
}

fn shown_dir(cfg: &Config) -> String {
    display_path(
        &cfg.data_dir(),
        &std::env::current_dir().unwrap_or_default(),
    )
}

fn show(via: &Via, args: &DataArgs, cfg: &Config, st: Style) -> Result<(), String> {
    let report = via.report(args.service.as_deref())?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
        return Ok(());
    }
    let Some(_) = &args.service else {
        print!(
            "{}",
            crate::data_show::overview(&report, &shown_dir(cfg), st)
        );
        return Ok(());
    };
    let svc = report
        .get("services")
        .and_then(Value::as_array)
        .and_then(|s| s.first())
        .ok_or("no such service")?;
    let Some(name) = &args.resource else {
        print!("{}", crate::data_show::service(svc, st));
        return Ok(());
    };
    let found: Vec<&Value> = svc
        .get("resources")
        .and_then(Value::as_array)
        .map(|rs| {
            rs.iter()
                .filter(|r| r.get("name").and_then(Value::as_str) == Some(name))
                .collect()
        })
        .unwrap_or_default();
    if found.is_empty() {
        let one = svc.get("one").and_then(Value::as_str).unwrap_or("resource");
        return Err(format!(
            "{} has no {one} named {name}",
            data::text(svc, "title")
        ));
    }
    for r in found {
        print!("{}", crate::data_show::resource(svc, r, st));
    }
    Ok(())
}

/// `--older-than 7d` as a time: now less the age.
pub(crate) fn before_of(age: &str) -> Result<i64, String> {
    let age = age.trim();
    let split = age.find(|c: char| !c.is_ascii_digit()).unwrap_or(age.len());
    let (n, unit) = age.split_at(split);
    let n: i64 = n.parse().map_err(|_| {
        format!("--older-than {age}: say a number and a unit, like 30m, 12h, 7d or 2w")
    })?;
    let secs = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        "w" => 604_800,
        _ => {
            return Err(format!(
                "--older-than {age}: the units are s, m, h, d and w (7d is seven days)"
            ));
        }
    };
    Ok(crate::data_show::now_ms() - n.saturating_mul(secs).saturating_mul(1000))
}

fn empty(via: &Via, e: &EmptyArgs, cfg: &Config, st: Style) -> Result<(), String> {
    let name = match (&e.name, e.unused) {
        (Some(n), _) => n.clone(),
        (None, true) => String::new(),
        (None, false) => {
            return Err(format!(
                "say which: doze-aws data empty {} <name>",
                e.service
            ));
        }
    };
    let prune = Prune {
        before_ms: e.older_than.as_deref().map(before_of).transpose()?,
        prefix: e.prefix.clone(),
        old_versions: e.old_versions,
        unused: e.unused,
        dry_run: true,
    };
    // `--region` names the resource's region only when it was given on the command line or in the environment for this.
    let region = cfg
        .get("region")
        .filter(|s| matches!(s.source, Source::Flag | Source::Env))
        .map(|s| s.value.join(","));
    let body = |region: Option<&str>, prune: &Prune| json!({"service": e.service, "name": name, "region": region, "prune": data::prune_json(prune)});
    let look = via.post("empty", &body(region.as_deref(), &prune))?;
    let found = data::text(&look, "region").to_owned();
    let (items, bytes) = (data::num(&look, "items"), data::num(&look, "bytes"));
    let what = describe_target(e, &name, &look);
    let units = data::text(&look, "units").to_owned();
    if items == 0 {
        println!("{what}: nothing to remove.");
        return Ok(());
    }
    println!("{what}: {} {units}, {}.", count(items), size(bytes));
    if e.dry_run {
        println!("{}", st.dim("Dry run: nothing was removed."));
        return Ok(());
    }
    if !confirm(e.yes, "Remove them? [y/N] ", "y")? {
        println!("Nothing was removed.");
        return Ok(());
    }
    // The region the look found, so the removal is of what was shown; `--unused` runs where it ran.
    let at = if e.unused {
        region.as_deref()
    } else {
        Some(found.as_str())
    };
    let done = via.post(
        "empty",
        &body(
            at,
            &Prune {
                dry_run: false,
                ..prune
            },
        ),
    )?;
    println!(
        "{} Removed {} {units} ({}); the disk got back {}.",
        st.green("✓"),
        count(data::num(&done, "items")),
        size(data::num(&done, "bytes")),
        size(data::num(&done, "freed")),
    );
    Ok(())
}

fn describe_target(e: &EmptyArgs, name: &str, look: &Value) -> String {
    let title = data::text(look, "title");
    let mut what = if e.unused {
        format!(
            "{title}: what nothing uses ({})",
            data::text(look, "region")
        )
    } else {
        format!(
            "{title} {} {name} ({})",
            data::text(look, "one"),
            data::text(look, "region")
        )
    };
    if let Some(p) = &e.prefix {
        what.push_str(&format!(", under {p}"));
    }
    if e.old_versions {
        what.push_str(", old versions and delete markers");
    }
    if let Some(a) = &e.older_than {
        what.push_str(&format!(", older than {a}"));
    }
    what
}

fn reset(via: &Via, r: &ResetArgs, cfg: &Config, st: Style) -> Result<(), String> {
    let look = via.post("reset", &json!({"service": r.service, "dry_run": true}))?;
    let report = via.report(r.service.as_deref())?;
    let services = |v: &Value| {
        v.get("services")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let (scope, on_disk) = match &r.service {
        Some(s) => (
            format!("everything {s} keeps"),
            services(&report)
                .first()
                .map_or(0, |p| data::num(p, "bytes")),
        ),
        None => (
            format!("everything doze-aws keeps in {}", shown_dir(cfg)),
            data::num(&report, "bytes"),
        ),
    };
    println!(
        "This removes {scope} ({}): resources, settings and data, as if it had never run.",
        size(on_disk)
    );
    for s in services(&look) {
        let items = data::num(&s, "items");
        if items > 0 {
            println!("  {:<16} {} items", data::text(&s, "service"), count(items));
        }
    }
    if r.dry_run {
        println!("{}", st.dim("Dry run: nothing was removed."));
        return Ok(());
    }
    let (prompt, word) = if r.service.is_some() {
        ("Remove it all? [y/N] ", "y")
    } else {
        ("Type reset to remove it all: ", "reset")
    };
    if !confirm(r.yes, prompt, word)? {
        println!("Nothing was removed.");
        return Ok(());
    }
    let done = via.post("reset", &json!({"service": r.service, "dry_run": false}))?;
    let freed: u64 = services(&done).iter().map(|s| data::num(s, "freed")).sum();
    println!("{} Done; the disk got back {}.", st.green("✓"), size(freed));
    Ok(())
}

/// Asks, unless told not to. Without a terminal to ask at, refuses rather than guess.
fn confirm(yes: bool, prompt: &str, word: &str) -> Result<bool, String> {
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        return Err("nothing was removed: there is no terminal to ask at. Add --yes to remove without asking.".to_owned());
    }
    print!("{prompt}");
    drop(std::io::stdout().flush());
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(line.trim().eq_ignore_ascii_case(word)
        || (word == "y" && line.trim().eq_ignore_ascii_case("yes")))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn an_age_is_a_number_and_a_unit() {
        let now = crate::data_show::now_ms();
        let week = before_of("7d").unwrap();
        assert!((now - week - 7 * 86_400_000).abs() < 5_000);
        assert!(before_of("2w").unwrap() < week);
        assert!(before_of("7").unwrap_err().contains("units"));
        assert!(before_of("d").unwrap_err().contains("number"));
        assert!(before_of("3y").unwrap_err().contains("units"));
    }
}
