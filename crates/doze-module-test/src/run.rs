//! Running a module's plan the way the core will: processes in `after` order as doze-os trees, each gated on its
//! readiness check, then its hooks; stopped in reverse order with the module's declared stop. Output goes to log files,
//! never pipes (W0: a tool that leaves a server behind keeps captured pipes open forever).

use doze_module_proto::msg::run::{Plan, Process, Ready, StopHow};
use doze_os::tree::{Stop, Stopped, Tree};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// The running processes of one plan.
#[derive(Debug)]
pub struct Started {
    procs: Vec<(Process, Tree)>,
    logs: PathBuf,
}

/// The order processes start in: each after the ones it names.
fn order(plan: &Plan) -> Result<Vec<&Process>, String> {
    let mut done: Vec<&Process> = Vec::new();
    while done.len() < plan.processes.len() {
        let next = plan.processes.iter().find(|p| {
            !done.iter().any(|d| d.name == p.name)
                && p.after.iter().all(|a| done.iter().any(|d| &d.name == a))
        });
        match next {
            Some(p) => done.push(p),
            None => {
                return Err("the plan's `after` names a missing process or makes a cycle".into());
            }
        }
    }
    Ok(done)
}

fn log_file(logs: &Path, name: &str) -> Result<std::fs::File, String> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join(format!("{name}.log")))
        .map_err(|e| e.to_string())
}

async fn run_to_end(argv: &[String]) -> Result<bool, String> {
    let (prog, args) = argv.split_first().ok_or("an empty command")?;
    let st = tokio::process::Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
    st.map(|s| s.success()).map_err(|e| format!("{prog}: {e}"))
}

async fn http_ok(url: &str) -> bool {
    let rest = url.strip_prefix("http://").unwrap_or(url);
    let (host, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
    let Ok(mut s) = tokio::net::TcpStream::connect(host).await else {
        return false;
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    if s.write_all(format!("GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n").as_bytes())
        .await
        .is_err()
    {
        return false;
    }
    let mut head = [0u8; 12];
    s.read_exact(&mut head).await.is_ok()
        && head.starts_with(b"HTTP/1.")
        && matches!(head.get(9), Some(b'2' | b'3'))
}

async fn ready_now(r: &Ready, log: &Path) -> Result<bool, String> {
    Ok(match r {
        Ready::Tcp(addr) => tokio::net::TcpStream::connect(addr).await.is_ok(),
        #[cfg(unix)]
        Ready::Socket(path) => tokio::net::UnixStream::connect(path).await.is_ok(),
        #[cfg(not(unix))]
        Ready::Socket(_) => {
            return Err(
                "a socket readiness check on a system without Unix sockets (use Tcp, PLAN.md D13)"
                    .into(),
            );
        }
        Ready::Http(url) => http_ok(url).await,
        Ready::Exec(argv) => run_to_end(argv).await?,
        Ready::LogLine(text) => {
            std::fs::read_to_string(log).is_ok_and(|s| s.contains(text.as_str()))
        }
    })
}

/// What a log says about an exit: from a Go panic's first line (its message, which a stack's tail never has),
/// else the last lines, in order.
fn why_it_exited(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let from = lines
        .iter()
        .rposition(|l| l.starts_with("panic:") || l.starts_with("fatal error:") || l.starts_with("Error:"))
        .unwrap_or(lines.len().saturating_sub(5));
    lines.iter().skip(from).take(8).copied().collect::<Vec<_>>().join(" | ")
}

async fn wait_ready(
    p: &Process,
    tree: &mut Tree,
    log: &Path,
    limit: Duration,
) -> Result<(), String> {
    let Some(ready) = &p.ready else {
        // No check: it must at least stay up a moment.
        tokio::time::sleep(Duration::from_millis(200)).await;
        return match tree.try_wait().map_err(|e| e.to_string())? {
            Some(st) => Err(format!("{} exited at once: {st}", p.name)),
            None => Ok(()),
        };
    };
    let t0 = Instant::now();
    loop {
        if let Some(st) = tree.try_wait().map_err(|e| e.to_string())? {
            let text = std::fs::read_to_string(log).unwrap_or_default();
            return Err(format!(
                "{} exited before it was ready ({st}): {}",
                p.name,
                why_it_exited(&text)
            ));
        }
        if ready_now(ready, log).await? {
            return Ok(());
        }
        if t0.elapsed() > limit {
            return Err(format!("{} not ready within {limit:?}", p.name));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Starts every process of `plan`, each ready before the next, running hooks after each.
pub async fn start(plan: &Plan, logs: &Path, limit: Duration) -> Result<Started, String> {
    let mut started = Started {
        procs: Vec::new(),
        logs: logs.to_path_buf(),
    };
    for p in order(plan)? {
        let log = logs.join(format!("{}.log", p.name));
        let (out, err) = (log_file(logs, &p.name)?, log_file(logs, &p.name)?);
        let mut cmd = tokio::process::Command::new(&p.program);
        cmd.args(&p.args)
            .envs(&p.env)
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err));
        if let Some(d) = &p.dir {
            cmd.current_dir(d);
        }
        let mut tree =
            Tree::spawn(cmd).map_err(|e| format!("starting {} ({}): {e}", p.name, p.program))?;
        let r = wait_ready(p, &mut tree, &log, limit).await;
        started.procs.push((p.clone(), tree));
        r?;
        for hook in &p.hooks {
            if !run_to_end(hook).await? {
                return Err(format!("{}: hook {hook:?} failed", p.name));
            }
        }
    }
    Ok(started)
}

impl Started {
    /// The log of one process so far.
    pub fn log(&self, name: &str) -> String {
        std::fs::read_to_string(self.logs.join(format!("{name}.log"))).unwrap_or_default()
    }

    /// Stops every process in reverse order, each the way its module declared; how each ended.
    pub async fn stop(mut self, grace: Duration) -> Vec<(String, std::io::Result<Stopped>)> {
        let mut out = Vec::new();
        while let Some((p, mut tree)) = self.procs.pop() {
            let how = match &p.stop {
                Some(StopHow::Interrupt) => Stop::Interrupt,
                Some(StopHow::Command(argv)) => {
                    Stop::Command(argv.iter().map(Into::into).collect())
                }
                Some(StopHow::Terminate) | None => Stop::Terminate,
            };
            out.push((p.name.clone(), tree.stop(&how, grace).await));
        }
        out
    }
}
