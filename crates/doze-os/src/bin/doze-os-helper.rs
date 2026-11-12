//! Test helper for doze-os's integration tests: one binary playing a module, an engine, or a privileged tool.
//! Frames on the channel are `[u32 LE length][u8 flags][payload]`; flag bit 0 = the payload ends with a handle trailer.
#![allow(clippy::print_stdout)]

use doze_os::channel::{self, TRAILER_LEN};
use doze_os::tree::Tree;
use std::io::Write as _;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => fail(&format!("runtime: {e}")),
    };
    let mode = args.first().map(String::as_str).unwrap_or("");
    let r = match mode {
        "module" => rt.block_on(module()),
        "sleep" => {
            std::thread::sleep(Duration::from_secs(3600));
            Ok(())
        }
        "wait-stop" => rt.block_on(wait_stop()),
        "ignore-stop" => rt.block_on(ignore_stop()),
        "host-tree" => rt.block_on(host_tree()),
        "privilege" => doze_os::privilege::is_privileged().map(|p| println!("privileged={p}")),
        "relaunch-privilege" => relaunch_then(|| {
            doze_os::privilege::is_privileged().map(|p| println!("privileged={p}"))
        }),
        "pg" => relaunch_then(|| rt.block_on(pg(args.get(1..).unwrap_or_default()))),
        _ => fail(&format!("unknown mode {mode:?}")),
    };
    if let Err(e) = r {
        fail(&e.to_string());
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("doze-os-helper: {msg}");
    std::process::exit(2);
}

fn relaunch_then(f: impl FnOnce() -> std::io::Result<()>) -> std::io::Result<()> {
    match doze_os::privilege::relaunch_unprivileged()? {
        Some(code) => std::process::exit(code),
        None => f(),
    }
}

/// Echoes plain frames; for a frame with a handle, writes the payload into the handed-over connection and answers
/// `ok:<payload>`. Exits 0 when the host goes away.
async fn module() -> std::io::Result<()> {
    let (mut r, mut w) = channel::module_channel()?.split();
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 64 << 10];
    loop {
        while let Some((flags, mut payload, used)) = parse(&buf) {
            buf.drain(..used);
            let reply = if flags & 1 == 1 {
                let at = payload.len().saturating_sub(TRAILER_LEN);
                let trailer = payload.split_off(at);
                let mut conn = std::net::TcpStream::from(r.import(&trailer)?);
                conn.write_all(&payload)?;
                drop(conn);
                [b"ok:".as_slice(), &payload].concat()
            } else {
                payload
            };
            w.write_all(&frame(0, &reply)).await?;
        }
        let n = r.read(&mut chunk).await?;
        if n == 0 {
            std::process::exit(0);
        }
        buf.extend_from_slice(chunk.get(..n).unwrap_or_default());
    }
}

fn parse(buf: &[u8]) -> Option<(u8, Vec<u8>, usize)> {
    let len = u32::from_le_bytes(buf.get(..4)?.try_into().ok()?) as usize;
    let flags = *buf.get(4)?;
    let payload = buf.get(5..5 + len)?.to_vec();
    Some((flags, payload, 5 + len))
}

fn frame(flags: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = (payload.len() as u32).to_le_bytes().to_vec();
    v.push(flags);
    v.extend_from_slice(payload);
    v
}

async fn wait_stop() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let (mut t, mut i) = (
            signal(SignalKind::terminate())?,
            signal(SignalKind::interrupt())?,
        );
        println!("ready");
        tokio::select! { _ = t.recv() => {}, _ = i.recv() => {} }
    }
    #[cfg(windows)]
    {
        let mut b = tokio::signal::windows::ctrl_break()?;
        println!("ready");
        b.recv().await;
    }
    std::process::exit(0);
}

async fn ignore_stop() -> std::io::Result<()> {
    #[cfg(unix)]
    let _keep = {
        use tokio::signal::unix::{SignalKind, signal};
        (
            signal(SignalKind::terminate())?,
            signal(SignalKind::interrupt())?,
        )
    };
    #[cfg(windows)]
    let _keep = tokio::signal::windows::ctrl_break()?;
    println!("ready");
    tokio::time::sleep(Duration::from_secs(3600)).await;
    Ok(())
}

/// A stand-in for doze: starts an engine tree, prints its pid, and waits to be killed.
async fn host_tree() -> std::io::Result<()> {
    let mut cmd = tokio::process::Command::new(std::env::current_exe()?);
    cmd.arg("sleep").stdin(std::process::Stdio::null());
    let tree = Tree::spawn(cmd)?;
    println!("{}", tree.pid().unwrap_or(0));
    std::io::stdout().flush()?;
    tokio::time::sleep(Duration::from_secs(3600)).await;
    drop(tree);
    Ok(())
}

/// Postgres, the way a module plus core will run it: initdb, the server as a tree started directly (no pg_ctl
/// launcher), ready by `pg_isready`, then the module's stop command (`pg_ctl stop -m fast`). Args: bindir, workdir.
async fn pg(args: &[String]) -> std::io::Result<()> {
    use doze_os::tree::Stop;
    let [bindir, work] = args else {
        return Err(std::io::Error::other("pg <bindir> <workdir>"));
    };
    let bin = |name: &str| {
        std::path::Path::new(bindir).join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
    };
    let data = std::path::Path::new(work).join("data");
    let quiet = |name: &str| {
        let mut c = tokio::process::Command::new(bin(name));
        c.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        c
    };
    initdb(&bin("initdb"), &data).await?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port()
        .to_string();
    let mut server = quiet("postgres");
    server
        .arg("-D")
        .arg(&data)
        .args(["-p", &port, "-c", "listen_addresses=127.0.0.1", "-k"])
        .arg(work);
    let mut tree = Tree::spawn(server)?;
    let t0 = std::time::Instant::now();
    loop {
        if let Some(st) = tree.try_wait()? {
            return Err(std::io::Error::other(format!(
                "postgres exited at once: {st}"
            )));
        }
        let ready = quiet("pg_isready")
            .args(["-h", "127.0.0.1", "-p", &port, "-U", "postgres"])
            .status()
            .await?;
        if ready.success() {
            break;
        }
        if t0.elapsed() > Duration::from_secs(60) {
            return Err(std::io::Error::other("postgres not ready in 60 s"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    println!("pg=ready in {} ms", t0.elapsed().as_millis());
    let stop = Stop::Command(vec![
        bin("pg_ctl").into(),
        "stop".into(),
        "-D".into(),
        data.into(),
        "-m".into(),
        "fast".into(),
        "-w".into(),
    ]);
    let r = tree.stop(&stop, Duration::from_secs(30)).await?;
    println!("pg=stopped {r:?}");
    Ok(())
}

/// initdb exits when done (it leaves no server behind), so its output can be captured for the failure message.
async fn initdb(initdb: &std::path::Path, data: &std::path::Path) -> std::io::Result<()> {
    let out = tokio::process::Command::new(initdb)
        .arg("-D")
        .arg(data)
        .args(["-U", "postgres", "-A", "trust", "--no-sync"])
        .stdin(std::process::Stdio::null())
        .output()
        .await?;
    if out.status.success() {
        return Ok(());
    }
    let why = [out.stdout, out.stderr].concat();
    let why = String::from_utf8_lossy(&why);
    Err(std::io::Error::other(format!(
        "initdb: {}: {}",
        out.status,
        why.trim()
    )))
}
