//! The echo module's engine: echoes every line back, on the address the host granted (`--listen unix:PATH` or
//! `--listen tcp:HOST:PORT`), and exits 0 when asked to stop (SIGTERM or SIGINT; CTRL_BREAK on Windows).

use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    // The engine's own client (what the module declares for `doze connect`): `client HOST PORT`.
    if let [_, mode, host, port] = args.as_slice()
        && mode == "client"
    {
        return match client(host, port) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("echo-engine client: {e}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    let Some(listen) = args
        .iter()
        .position(|a| a == "--listen")
        .and_then(|i| args.get(i + 1))
        .cloned()
    else {
        eprintln!("echo-engine: --listen unix:PATH | tcp:ADDR");
        return std::process::ExitCode::FAILURE;
    };
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return std::process::ExitCode::FAILURE;
    };
    match rt.block_on(run(&listen)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("echo-engine: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Sends stdin to `host:port` and prints what comes back, until stdin ends and the engine has answered. A password, when
/// the instance has one, comes in `ECHO_PASSWORD` (never argv); it is said, not shown.
fn client(host: &str, port: &str) -> std::io::Result<()> {
    use std::io::{Read, Write};
    if std::env::var_os("ECHO_PASSWORD").is_some() {
        eprintln!("echo-engine client: signed in");
    }
    let mut conn = std::net::TcpStream::connect(format!("{host}:{port}"))?;
    let mut back = conn.try_clone()?;
    let reader = std::thread::spawn(move || -> std::io::Result<()> {
        let mut buf = [0u8; 4096];
        let mut out = std::io::stdout();
        loop {
            let n = back.read(&mut buf)?;
            let Some(got) = buf.get(..n).filter(|b| !b.is_empty()) else {
                return Ok(());
            };
            out.write_all(got)?;
            out.flush()?;
        }
    });
    std::io::copy(&mut std::io::stdin(), &mut conn)?;
    // Half-closed: the engine answers what was sent, then ends the line, which ends the reader.
    conn.shutdown(std::net::Shutdown::Write)?;
    reader
        .join()
        .map_err(|_| std::io::Error::other("the reader panicked"))?
}

async fn echo<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(s: S) {
    let mut s = BufReader::new(s);
    let mut line = String::new();
    while matches!(s.read_line(&mut line).await, Ok(n) if n > 0) {
        if s.get_mut().write_all(line.as_bytes()).await.is_err() {
            return;
        }
        line.clear();
    }
}

async fn serve(listen: &str) -> std::io::Result<()> {
    if let Some(addr) = listen.strip_prefix("tcp:") {
        let l = tokio::net::TcpListener::bind(addr).await?;
        loop {
            let (s, _) = l.accept().await?;
            tokio::spawn(echo(s));
        }
    }
    #[cfg(unix)]
    if let Some(path) = listen.strip_prefix("unix:") {
        let _ = std::fs::remove_file(path);
        let l = tokio::net::UnixListener::bind(path)?;
        loop {
            let (s, _) = l.accept().await?;
            tokio::spawn(echo(s));
        }
    }
    Err(std::io::Error::other(format!("cannot listen on {listen}")))
}

async fn stopped() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let (mut t, mut i) = (
            signal(SignalKind::terminate())?,
            signal(SignalKind::interrupt())?,
        );
        tokio::select! { _ = t.recv() => {}, _ = i.recv() => {} }
    }
    #[cfg(windows)]
    tokio::signal::windows::ctrl_break()?.recv().await;
    Ok(())
}

async fn run(listen: &str) -> std::io::Result<()> {
    tokio::select! {
        r = serve(listen) => r,
        r = stopped() => {
            // A moment for in-flight echoes, as a real engine would flush.
            tokio::time::sleep(Duration::from_millis(10)).await;
            r
        }
    }
}
