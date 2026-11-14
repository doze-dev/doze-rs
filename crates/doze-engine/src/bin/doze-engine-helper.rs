//! Test helper for doze-engine: one binary playing a supervised command or a host that runs an engine.
//!
//! - `listen [delay_ms]`: after the delay, serves HTTP 200 on `$PORT` until asked to stop (or `$STOP_FILE` appears),
//!   then exits 0. Each start appends a line to `$STARTS` when set.
//! - `exit <code> [after_ms]`: exits with the code.
//! - `grandchild <file>`: starts a child of its own (same tree), writes the child's pid to the file, then serves.
//! - `forklisten`: starts a child that serves (and prints its port); the parent itself listens on nothing.
//! - `ignore`: ignores being asked to stop.
//! - `slowchild <file>`: starts a child that ignores signals, writes its pid to the file, then never gets ready.
//! - `crashchild <file> <code> <after_ms>`: starts a child that ignores signals, appends its pid to the file, then
//!   exits with the code after the delay.
//! - `check <file>`: exits 0 if the file exists, else 1. `append <file>`: appends a line to the file.
//! - `host <data_dir> [running|readiness|setup]`: runs an engine with one service and wakes it; once the moment is
//!   reached (running; waiting for readiness; in a `setup` step) prints `pids <recorded> <grandchild>` and waits to be
//!   killed: the "nothing left behind after the host is killed" tests.
#![allow(clippy::print_stdout)]

use std::io::Write as _;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => fail(&format!("runtime: {e}")),
    };
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    let ms = |i: usize| Duration::from_millis(arg(i).parse().unwrap_or(0));
    let r = match arg(0).as_str() {
        "listen" => rt.block_on(listen(ms(1))),
        "exit" => {
            std::thread::sleep(ms(2));
            std::process::exit(arg(1).parse().unwrap_or(1));
        }
        "grandchild" => rt.block_on(grandchild(&arg(1))),
        "ignore" => rt.block_on(ignore()),
        "slowchild" => rt.block_on(slowchild(&arg(1))),
        "crashchild" => {
            if let Err(e) = start_ignoring_child(&arg(1), true) {
                fail(&e.to_string());
            }
            std::thread::sleep(ms(3));
            std::process::exit(arg(2).parse().unwrap_or(1));
        }
        "say" => {
            // `say <n> <code> <prefix>`: n lines "<prefix> <i>", then exit with code.
            let n: u32 = arg(1).parse().unwrap_or(0);
            for i in 1..=n {
                println!("{} {i}", arg(3));
            }
            std::process::exit(arg(2).parse().unwrap_or(1));
        }
        "partial" => {
            // `partial <text> <code>`: <text> to stderr with no newline, then exit with code (`hang`: stay up).
            partial(&arg(1), arg(2) == "hang", &rt);
            std::process::exit(arg(2).parse().unwrap_or(1));
        }
        "escape" => rt.block_on(escape(&arg(1))),
        "forklisten" => fork_listen(),
        "chatter" => {
            for i in 0..300 {
                println!("escaped says {i}");
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(())
        }
        "envto" => {
            // `envto <file> <KEY>`: appends the variable's value (or "unset") to the file.
            let v = std::env::var(arg(2)).unwrap_or_else(|_| "unset".into());
            append_line(&arg(1), &v)
        }
        "check" => std::process::exit(i32::from(!std::path::Path::new(&arg(1)).exists())),
        "append" => append(&arg(1)),
        "host" => rt.block_on(crate::host::host(&arg(1), &arg(2))),
        other => fail(&format!("unknown mode {other:?}")),
    };
    if let Err(e) = r {
        fail(&e.to_string());
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("doze-engine-helper: {msg}");
    std::process::exit(2);
}

async fn listen(delay: Duration) -> std::io::Result<()> {
    if let Ok(f) = std::env::var("STARTS") {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(f)?;
        writeln!(f, "{}", std::process::id())?;
    }
    tokio::time::sleep(delay).await;
    // `LISTEN_ON` overrides `$PORT`: an app that ignores the port doze gave it (the port trap).
    let port = std::env::var("LISTEN_ON")
        .or_else(|_| std::env::var("PORT"))
        .unwrap_or_else(|_| "0".into());
    let l = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    println!("listening on {}", l.local_addr()?.port());
    let serve = async {
        loop {
            if let Ok((mut s, _)) = l.accept().await {
                tokio::spawn(async move {
                    let _ = s
                        .write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\nok")
                        .await;
                });
            }
        }
    };
    let stop_file = async {
        let Ok(f) = std::env::var("STOP_FILE") else {
            return std::future::pending().await;
        };
        while !std::path::Path::new(&f).exists() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    };
    tokio::select! {
        r = doze_os::signal::stop_requested() => r?,
        () = serve => {},
        () = stop_file => {},
    }
    println!("stopping");
    std::process::exit(0);
}

/// Starts a child that ignores signals and records its pid in `file` (appended, or written whole). The child is left
/// running on purpose: the tree must take it down.
fn start_ignoring_child(file: &str, append: bool) -> std::io::Result<()> {
    let child = std::process::Command::new(std::env::current_exe()?)
        .arg("ignore")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn()?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(file)?;
    writeln!(f, "{}", child.id())?;
    std::mem::forget(child);
    Ok(())
}

/// `escape <pidfile>`: starts a chatter that leaves the group (a new session) holding stdout, records its pid, then
/// listens like `listen`.
async fn escape(pidfile: &str) -> std::io::Result<()> {
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.arg("chatter").stdin(std::process::Stdio::null());
    let pid = doze_os::detach::spawn_detached(cmd)?;
    std::fs::write(pidfile, pid.to_string())?;
    listen(Duration::ZERO).await
}

/// `forklisten`: a parent that does not listen, with a child that does (`listen` on `$LISTEN_ON`, printing its port on
/// the shared stdout). The parent exits when the child does (`$STOP_FILE`).
fn fork_listen() -> std::io::Result<()> {
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .arg("listen")
        .stdin(std::process::Stdio::null())
        .spawn()?;
    child.wait().map(drop)
}

async fn grandchild(file: &str) -> std::io::Result<()> {
    start_ignoring_child(file, false)?;
    listen(Duration::ZERO).await
}

async fn slowchild(file: &str) -> std::io::Result<()> {
    start_ignoring_child(file, false)?;
    tokio::time::sleep(Duration::from_secs(3600)).await;
    Ok(())
}

fn append_line(file: &str, line: &str) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)?;
    writeln!(f, "{line}")
}

fn append(file: &str) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)?;
    writeln!(f, "ran")
}

async fn ignore() -> std::io::Result<()> {
    let keep = tokio::spawn(async {
        loop {
            let _ = doze_os::signal::stop_requested().await;
        }
    });
    tokio::time::sleep(Duration::from_secs(3600)).await;
    keep.abort();
    Ok(())
}

#[path = "helper/host.rs"]
mod host;

/// `text` to stderr with no newline; with `hang`, stays up after (its last words are only in the pump until its pipe
/// closes).
fn partial(text: &str, hang: bool, rt: &tokio::runtime::Runtime) {
    eprint!("{text}");
    let _ = std::io::Write::flush(&mut std::io::stderr());
    if hang {
        let _ = rt.block_on(ignore());
    }
}
