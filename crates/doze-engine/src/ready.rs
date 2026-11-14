//! Readiness and liveness probes (Go's `internal/health`): one check of a [`Ready`], and the wait that gates a start.
//!
//! The wait probes at once, then backs off from 10 ms to 100 ms, so a backend ready in 30 ms is seen in about 30 ms (a
//! whole interval after the first miss put a one-second floor under every wake in Go once). A process with no check
//! must still be running after 750 ms.

use crate::logs::RunLog;
use crate::pidfile::Pids;
use doze_module_proto::msg::run::Ready;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The first wait after a failed probe; it doubles up to [`MAX_BACKOFF`].
const FIRST_BACKOFF: Duration = Duration::from_millis(10);
const MAX_BACKOFF: Duration = Duration::from_millis(100);
/// How long a process with no check must stay up.
pub const LIVENESS_GRACE: Duration = Duration::from_millis(750);
/// One probe's own limit.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// One loopback connect's limit: a listener answers in well under a millisecond, and Windows retries a refused
/// connection for about 2 s before it says so, which would put that under every probe.
const CONNECT_LIMIT: Duration = Duration::from_millis(250);
/// How many lines of the log a `log_line` probe reads (Go: the newest run's last 200).
const LOG_LINES: usize = 200;

/// Why a start was not ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotReady {
    /// No check, and it was gone after the grace period.
    ExitedImmediately,
    /// It exited while being probed.
    ExitedDuringStartup,
    /// The budget ran out.
    Timeout(Duration),
    /// The check itself is broken (a bad regular expression, a socket check where there are none).
    Broken(String),
}

impl std::fmt::Display for NotReady {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotReady::ExitedImmediately => f.write_str("exited immediately"),
            NotReady::ExitedDuringStartup => f.write_str("exited during startup"),
            NotReady::Timeout(d) => {
                write!(
                    f,
                    "did not become ready within {}",
                    crate::godur::go_duration(*d)
                )
            }
            NotReady::Broken(m) => f.write_str(m),
        }
    }
}

/// Waits until `ready` passes, the process dies (`alive` says), or `budget` is spent. `log` is the run a `log_line`
/// check reads (none: it never passes); an exec probe is recorded in `pids` while it runs.
pub async fn wait_ready(
    ready: Option<&Ready>,
    budget: Duration,
    log: Option<&RunLog>,
    pids: Option<&Pids>,
    mut alive: impl FnMut() -> bool,
) -> Result<(), NotReady> {
    let Some(ready) = ready else {
        tokio::time::sleep(LIVENESS_GRACE).await;
        return if alive() {
            Ok(())
        } else {
            Err(NotReady::ExitedImmediately)
        };
    };
    let deadline = Instant::now() + budget;
    let mut wait = FIRST_BACKOFF;
    loop {
        if !alive() {
            return Err(NotReady::ExitedDuringStartup);
        }
        if probe(ready, log, pids).await? {
            return Ok(());
        }
        if Instant::now() + wait >= deadline {
            return Err(NotReady::Timeout(budget));
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(MAX_BACKOFF);
    }
}

/// One check: `Ok(true)` passed, `Ok(false)` not yet, `Err` the check cannot work. An exec probe is recorded in
/// `pids` while it runs.
pub async fn probe(
    ready: &Ready,
    log: Option<&RunLog>,
    pids: Option<&Pids>,
) -> Result<bool, NotReady> {
    let once = async {
        match ready {
            Ready::Tcp(addr) => Ok(matches!(
                tokio::time::timeout(CONNECT_LIMIT, doze_os::net::connect_tcp(addr)).await,
                Ok(Ok(_))
            )),
            Ready::Socket(path) => match doze_os::net::connect_unix(path).await {
                Ok(_) => Ok(true),
                Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
                    Err(NotReady::Broken(e.to_string()))
                }
                Err(_) => Ok(false),
            },
            Ready::Http(url) => Ok(http_ok(url).await),
            Ready::Exec(argv) => Ok(exec_ok(argv, pids).await),
            Ready::LogLine(pattern) => log_has(pattern, log),
        }
    };
    tokio::time::timeout(PROBE_TIMEOUT, once)
        .await
        .unwrap_or(Ok(false))
}

/// A GET that answers 2xx. Go's client followed redirects to the final answer; a redirect counts as ready here.
async fn http_ok(url: &str) -> bool {
    let rest = url.strip_prefix("http://").unwrap_or(url);
    let (host, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
    let Ok(Ok(mut s)) =
        tokio::time::timeout(CONNECT_LIMIT, tokio::net::TcpStream::connect(host)).await
    else {
        return false;
    };
    let req = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).await.is_err() {
        return false;
    }
    let mut head = [0u8; 12];
    s.read_exact(&mut head).await.is_ok()
        && head.starts_with(b"HTTP/1.")
        && matches!(head.get(9), Some(b'2' | b'3'))
}

/// A command (no shell, PLAN.md D14) that exits 0, run as a tree that goes when the probe does.
pub(crate) async fn exec_ok(argv: &[String], pids: Option<&Pids>) -> bool {
    let short = crate::short::Short {
        argv,
        env: None,
        dir: None,
        out: None,
        capture: None,
    };
    crate::short::run(short, PROBE_TIMEOUT, pids).await.is_ok()
}

/// Whether one of this run's last [`LOG_LINES`] lines matches (the store reads them backwards from the end, so a
/// probe every 10-100 ms costs what it reads, not the log's size).
fn log_has(pattern: &str, log: Option<&RunLog>) -> Result<bool, NotReady> {
    let re = regex_lite::Regex::new(pattern)
        .map_err(|e| NotReady::Broken(format!("invalid log_line regex {pattern:?}: {e}")))?;
    Ok(log.is_some_and(|l| l.recent(LOG_LINES).iter().any(|t| re.is_match(t))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tcp_passes_when_something_listens() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let r = Ready::Tcp(addr.clone());
        assert_eq!(probe(&r, None, None).await, Ok(true));
        drop(l);
        // Port 1 on loopback: nothing listens (a dropped ephemeral port may be re-bound by a parallel test).
        let closed = Ready::Tcp("127.0.0.1:1".into());
        assert_eq!(probe(&closed, None, None).await, Ok(false));
    }

    fn run_log(name: &str, lines: &[&str]) -> (tempfile::TempDir, RunLog) {
        let d = tempfile::tempdir().unwrap();
        let store = doze_logstore::Store::open(d.path());
        let log = RunLog::begin(&store, name).unwrap();
        for l in lines {
            log.line("o", l);
        }
        (d, log)
    }

    #[tokio::test]
    async fn log_line_is_a_regular_expression_over_this_runs_lines() {
        let (_d, log) = run_log("x", &["booting", "ready to accept connections on 5432"]);
        let ok = Ready::LogLine("accept.*5432".into());
        assert_eq!(probe(&ok, Some(&log), None).await, Ok(true));
        let no = Ready::LogLine("^listening".into());
        assert_eq!(probe(&no, Some(&log), None).await, Ok(false));
        assert_eq!(probe(&ok, None, None).await, Ok(false));
        let bad = Ready::LogLine("(".into());
        assert!(matches!(
            probe(&bad, Some(&log), None).await,
            Err(NotReady::Broken(_))
        ));
        // A new run: the last one's lines no longer count.
        let next = RunLog::begin(log.run().store(), "x").unwrap();
        assert_eq!(probe(&ok, Some(&next), None).await, Ok(false));
    }

    #[tokio::test]
    async fn a_log_line_probe_reads_only_the_last_200_lines() {
        // 200 lines are read: the marker 200 back is, the one 201 back is not (Go's `recentLogs`).
        let mut lines = vec!["early marker", "edge marker"];
        lines.extend(std::iter::repeat_n("filler", 198));
        lines.push("late marker");
        let (_d, log) = run_log("x", &lines);
        let late = Ready::LogLine("^late marker$".into());
        assert_eq!(probe(&late, Some(&log), None).await, Ok(true));
        let edge = Ready::LogLine("^edge marker$".into());
        assert_eq!(probe(&edge, Some(&log), None).await, Ok(true));
        let early = Ready::LogLine("^early marker$".into());
        assert_eq!(probe(&early, Some(&log), None).await, Ok(false));
    }

    #[tokio::test]
    async fn a_fast_start_is_seen_fast_and_the_budget_is_kept() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let r = Ready::Tcp(l.local_addr().unwrap().to_string());
        let t = Instant::now();
        assert_eq!(
            wait_ready(Some(&r), Duration::from_secs(5), None, None, || true).await,
            Ok(())
        );
        assert!(t.elapsed() < Duration::from_millis(200));
        drop(l);
        let r = Ready::Tcp("127.0.0.1:1".into());
        let t = Instant::now();
        let got = wait_ready(Some(&r), Duration::from_millis(300), None, None, || true).await;
        assert_eq!(got, Err(NotReady::Timeout(Duration::from_millis(300))));
        assert!(t.elapsed() < Duration::from_millis(600));
        assert_eq!(
            got.err().map(|e| e.to_string()).unwrap_or_default(),
            "did not become ready within 300ms"
        );
    }

    #[tokio::test]
    async fn liveness_and_death_during_startup() {
        let r = Ready::Tcp("127.0.0.1:1".into());
        let got = wait_ready(Some(&r), Duration::from_secs(5), None, None, || false).await;
        assert_eq!(got, Err(NotReady::ExitedDuringStartup));
        assert_eq!(
            wait_ready(None, Duration::ZERO, None, None, || false).await,
            Err(NotReady::ExitedImmediately)
        );
        assert_eq!(
            wait_ready(None, Duration::ZERO, None, None, || true).await,
            Ok(())
        );
    }

    #[tokio::test]
    async fn exec_and_http() {
        assert!(!exec_ok(&[], None).await);
        assert!(!exec_ok(&["doze-no-such-program-here".into()], None).await);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut s, _)) = l.accept().await {
                let mut buf = [0u8; 256];
                let _ = s.read(&mut buf).await;
                let _ = s.write_all(b"HTTP/1.1 204 No Content\r\n\r\n").await;
            }
        });
        assert!(http_ok(&format!("http://{addr}/health")).await);
    }
}
