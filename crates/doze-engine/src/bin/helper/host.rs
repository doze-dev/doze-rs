//! `host <data_dir> <moment>`: a stand-in daemon running an engine, killed outright by the test once `moment` is
//! reached. It prints `pids <recorded> <grandchild>`: the pid the engine recorded for what runs at that moment, and a
//! grandchild that ignores signals.

use doze_engine::{Cause, Engine, ProcessSpec, Service, Step};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) async fn host(data_dir: &str, moment: &str) -> std::io::Result<()> {
    let dir = Path::new(data_dir);
    let exe = std::env::current_exe()?.to_string_lossy().into_owned();
    let gc = dir.join("grandchild.pid");
    let gcs = gc.to_string_lossy().into_owned();
    let mut spec = match moment {
        "readiness" => {
            let mut s = ProcessSpec::new([exe.as_str(), "slowchild", &gcs], data_dir).port(1);
            s.ready_timeout = Duration::from_secs(600);
            s
        }
        "setup" => {
            let mut s = ProcessSpec::new([exe.as_str(), "listen"], data_dir).port(1);
            s.setup = vec![Step::argv([exe.as_str(), "slowchild", &gcs])];
            s
        }
        _ => ProcessSpec::new([exe.as_str(), "grandchild", &gcs], data_dir).port(1),
    };
    spec.restart.max_retries = 0;
    let engine = Engine::builder("host", data_dir)
        .service(Service::process("web", spec))
        .build()
        .map_err(std::io::Error::other)?;
    let woke = {
        let e = engine.clone();
        tokio::spawn(async move { e.wake("web", Cause::Command).await })
    };
    let run = dir.join("run");
    let recorded = loop {
        let pid = match moment {
            "setup" => step_pid(&run.join("backend-web.pids")),
            _ => first_line(&run.join("backend-web.pid")),
        };
        if pid > 0 && first_line(&gc) > 0 {
            break pid;
        }
        if woke.is_finished() && moment != "running" {
            return Err(std::io::Error::other("the wake ended before the moment"));
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    println!("pids {recorded} {}", first_line(&gc));
    std::io::stdout().flush()?;
    tokio::time::sleep(Duration::from_secs(3600)).await;
    Ok(())
}

fn first_line(file: &Path) -> u32 {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|s| s.lines().next().and_then(|l| l.trim().parse().ok()))
        .unwrap_or(0)
}

/// The pid of a step running now (`run-<pid>.pid`).
fn step_pid(folder: &Path) -> u32 {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p: &PathBuf| p.to_string_lossy().contains("run-"))
        .map(|p| first_line(&p))
        .find(|p| *p > 0)
        .unwrap_or(0)
}
