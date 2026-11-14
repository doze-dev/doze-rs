//! Test helper for doze-engine: one binary playing a supervised command or a host that runs an engine.
//!
//! - `listen [delay_ms]`: after the delay, serves HTTP 200 on `$PORT` until asked to stop, then exits 0. Each start
//!   appends a line to `$STARTS` when set.
//! - `exit <code> [after_ms]`: exits with the code.
//! - `grandchild <file>`: starts a child of its own (same tree), writes the child's pid to the file, then serves.
//! - `ignore`: ignores being asked to stop.
//! - `check <file>`: exits 0 if the file exists, else 1. `append <file>`: appends a line to the file.
//! - `host <data_dir>`: runs an engine with one service (`grandchild`), wakes it, prints `pids <a> <b>`, and waits to
//!   be killed: the "nothing left behind after the host is killed" test.
#![allow(clippy::print_stdout)]

use doze_engine::{Cause, Engine, ProcessSpec, Service};
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
        "check" => std::process::exit(i32::from(!std::path::Path::new(&arg(1)).exists())),
        "append" => append(&arg(1)),
        "host" => rt.block_on(host(&arg(1))),
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
    let port = std::env::var("PORT").unwrap_or_else(|_| "0".into());
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
    tokio::select! {
        r = doze_os::signal::stop_requested() => r?,
        () = serve => {},
    }
    println!("stopping");
    std::process::exit(0);
}

async fn grandchild(file: &str) -> std::io::Result<()> {
    let child = std::process::Command::new(std::env::current_exe()?)
        .arg("ignore")
        .stdin(std::process::Stdio::null())
        .spawn()?;
    std::fs::write(file, child.id().to_string())?;
    // The child is left running on purpose: the tree must take it down.
    std::mem::forget(child);
    listen(Duration::ZERO).await
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

async fn host(data_dir: &str) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let gc = std::path::Path::new(data_dir).join("grandchild.pid");
    let run = [
        exe.to_string_lossy().into_owned(),
        "grandchild".into(),
        gc.to_string_lossy().into_owned(),
    ];
    let spec = ProcessSpec::new(run, data_dir).port(1);
    let engine = Engine::builder("host", data_dir)
        .service(Service::process("web", spec))
        .build()
        .map_err(std::io::Error::other)?;
    engine
        .wake("web", Cause::Command)
        .await
        .map_err(std::io::Error::other)?;
    let pid = engine.get("web").map(|i| i.pid).unwrap_or(0);
    let grand = std::fs::read_to_string(&gc).unwrap_or_default();
    println!("pids {pid} {}", grand.trim());
    std::io::stdout().flush()?;
    tokio::time::sleep(Duration::from_secs(3600)).await;
    Ok(())
}
