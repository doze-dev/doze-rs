//! `doze-aws doctor`: what could get in the way of the configured doze-aws, one verdict a line, and the command that fixes each.
//! Unprivileged and read-only (the one write is a probe file in the data directory, removed at once). It exits non-zero when a
//! check fails, so a script can gate on it.

use std::net::{SocketAddr, TcpListener};
use std::path::Path;

use crate::config::{Config, display_path};
use crate::instance::{Instance, endpoint_of};
use crate::style::{Stream, Style, Verdict, pad};

/// One check's finding.
#[derive(Debug)]
pub(crate) struct Check {
    pub(crate) verdict: Verdict,
    pub(crate) name: &'static str,
    pub(crate) detail: String,
    pub(crate) fix: Option<String>,
}

fn check(verdict: Verdict, name: &'static str, detail: String, fix: Option<String>) -> Check {
    Check {
        verdict,
        name,
        detail,
        fix,
    }
}

/// Runs the command; the exit code.
pub(crate) fn run(cfg: &Config) -> i32 {
    let running = crate::locate::find(cfg);
    let env: Vec<(String, String)> = std::env::vars().collect();
    let mut checks = vec![running_check(running.as_ref())];
    checks.push(port_check(cfg, running.as_ref()));
    checks.push(data_check(cfg, running.as_ref()));
    checks.extend(disk_check(&cfg.data_dir()));
    checks.extend(shell_checks(&env, running.as_ref()));
    print(&checks, cfg, Style::of(Stream::Out));
    i32::from(checks.iter().any(|c| c.verdict == Verdict::Fail))
}

fn print(checks: &[Check], cfg: &Config, s: Style) {
    println!(
        "\n  {} {}",
        s.bold("doze-aws doctor"),
        s.dim(crate::VERSION)
    );
    if let Some(f) = &cfg.file {
        println!("  {}", s.dim(&format!("config: {}", f.display())));
    }
    println!();
    for c in checks {
        println!(
            "    {}  {}  {}",
            s.mark(c.verdict),
            pad(c.name, 9),
            c.detail
        );
        if let Some(fix) = &c.fix {
            println!("        {}  {}", pad("", 9), s.cyan(fix));
        }
    }
    let bad = checks.iter().filter(|c| c.verdict != Verdict::Ok).count();
    match bad {
        0 => println!("\n  {}\n", s.green("Everything is in order.")),
        1 => println!("\n  One thing to look at.\n"),
        n => println!("\n  {n} things to look at.\n"),
    }
}

fn running_check(running: Option<&Instance>) -> Check {
    let Some(i) = running else {
        return check(
            Verdict::Warn,
            "running",
            "no doze-aws is running for this directory".into(),
            Some("doze-aws".into()),
        );
    };
    if crate::locate::health(&i.endpoint).is_none() {
        let detail = format!(
            "pid {} holds the data directory but does not answer at {}",
            i.pid, i.endpoint
        );
        return check(
            Verdict::Fail,
            "running",
            detail,
            Some(format!("kill {}   # then start it again: doze-aws", i.pid)),
        );
    }
    let detail = format!(
        "pid {} at {}, {} services in {}",
        i.pid,
        i.endpoint,
        i.services.len(),
        i.region
    );
    check(Verdict::Ok, "running", detail, None)
}

fn port_check(cfg: &Config, running: Option<&Instance>) -> Check {
    let text = cfg.text("listen");
    let Ok(addr) = text.parse::<SocketAddr>() else {
        return check(
            Verdict::Fail,
            "port",
            format!("{text} is not an address"),
            None,
        );
    };
    if addr.port() == 0 {
        return check(
            Verdict::Ok,
            "port",
            "any free port (--listen with port 0)".into(),
            None,
        );
    }
    match running {
        Some(i) if i.endpoint == endpoint_of(addr) => {
            return check(
                Verdict::Ok,
                "port",
                format!("{addr} is this doze-aws"),
                None,
            );
        }
        Some(i) => {
            let detail = format!(
                "this doze-aws is on {} (the configured {addr} is not used)",
                i.endpoint
            );
            return check(Verdict::Ok, "port", detail, None);
        }
        None => {}
    }
    if TcpListener::bind(addr).is_ok() {
        return check(Verdict::Ok, "port", format!("{addr} is free"), None);
    }
    if let Some(other) = crate::locate::health(&endpoint_of(addr)) {
        let detail = format!(
            "{addr} is another doze-aws (pid {}, data {})",
            other.pid, other.data_dir
        );
        return check(
            Verdict::Warn,
            "port",
            detail,
            Some("doze-aws --listen 127.0.0.1:0   # or use that one".into()),
        );
    }
    let who = crate::probe::port_owner(addr.port()).map_or_else(
        || "another program".to_owned(),
        |(pid, cmd)| format!("{cmd} (pid {pid})"),
    );
    let fix = format!("doze-aws --listen 127.0.0.1:0   # or stop {who}");
    check(
        Verdict::Fail,
        "port",
        format!("{addr} is taken by {who}"),
        Some(fix),
    )
}

fn data_check(cfg: &Config, running: Option<&Instance>) -> Check {
    let dir = cfg.data_dir();
    let shown = display_path(&dir, &std::env::current_dir().unwrap_or_default());
    if let Some(i) = running.filter(|i| Path::new(&i.data_dir) == dir) {
        return check(
            Verdict::Ok,
            "data",
            format!("{shown}, held by the running doze-aws (pid {})", i.pid),
            None,
        );
    }
    if crate::instance::held(&dir) {
        let detail = format!("{shown} is held by another process");
        return check(
            Verdict::Fail,
            "data",
            detail,
            Some("doze-aws --data-dir <another directory>".into()),
        );
    }
    let probe_dir = dir.ancestors().find(|p| p.exists()).unwrap_or(&dir);
    let probe = probe_dir.join(format!(".doze-aws-probe-{}", std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            let state = if dir.exists() {
                "writable"
            } else {
                "will be made"
            };
            check(Verdict::Ok, "data", format!("{shown} ({state})"), None)
        }
        Err(e) => check(
            Verdict::Fail,
            "data",
            format!("{shown} cannot be written: {e}"),
            Some("doze-aws --data-dir <a directory you own>".into()),
        ),
    }
}

fn disk_check(dir: &Path) -> Option<Check> {
    let free = crate::probe::free_bytes(dir)?;
    let detail = format!("{} free", crate::probe::human(free));
    Some(match free {
        f if f < 200 << 20 => check(
            Verdict::Fail,
            "disk",
            detail,
            Some("free some space; S3 objects and logs live in the data directory".into()),
        ),
        f if f < 2 << 30 => check(Verdict::Warn, "disk", detail, None),
        _ => check(Verdict::Ok, "disk", detail, None),
    })
}

/// What this shell's AWS variables would do to a call: go to real AWS, to another address, or skip the endpoint altogether.
pub(crate) fn shell_checks(env: &[(String, String)], running: Option<&Instance>) -> Vec<Check> {
    let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let fix = Some("eval \"$(doze-aws env)\"".to_owned());
    let mut out = Vec::new();
    match (get("AWS_ENDPOINT_URL"), running) {
        (None, _) => out.push(check(
            Verdict::Warn,
            "shell",
            "AWS_ENDPOINT_URL is not set: SDKs in this shell call real AWS".into(),
            fix.clone(),
        )),
        (Some(url), Some(i)) if url.trim_end_matches('/') != i.endpoint => {
            out.push(check(
                Verdict::Warn,
                "shell",
                format!(
                    "AWS_ENDPOINT_URL is {url}, not this doze-aws ({})",
                    i.endpoint
                ),
                fix.clone(),
            ));
        }
        (Some(url), _) => out.push(check(
            Verdict::Ok,
            "shell",
            format!("AWS_ENDPOINT_URL is {url}"),
            None,
        )),
    }
    let extra = crate::env::interfering(env);
    if !extra.is_empty() {
        let detail = format!(
            "{} set: it sends calls past AWS_ENDPOINT_URL or adds a real session",
            extra.join(", ")
        );
        out.push(check(Verdict::Warn, "shell", detail, fix.clone()));
    }
    if let (Some(region), Some(i)) = (
        get("AWS_REGION").or_else(|| get("AWS_DEFAULT_REGION")),
        running,
    ) && region != i.region
    {
        let detail = format!(
            "AWS_REGION is {region}; doze-aws defaults to {} (both work, but resources are per region)",
            i.region
        );
        out.push(check(Verdict::Warn, "shell", detail, fix));
    }
    out
}

#[cfg(test)]
#[path = "doctor_tests.rs"]
mod tests;
