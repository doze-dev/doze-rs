//! S0 spike driver. `s0 host` runs every transport scenario against a child `s0 module`;
//! `s0 orphan` checks the module dies with its host; `s0 bench` compares codecs.

mod bench;

use anyhow::{Context, Result, anyhow, bail};
use s0_module_proto::m;
use s0_module_proto::msg::*;
use s0_module_proto::wire::{Handler, Incoming, Peer};
use std::os::fd::{AsFd, OwnedFd};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("");
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    match cmd {
        "module" => rt.block_on(module_main()),
        "host" => rt.block_on(host_main()),
        "host-hold" => rt.block_on(host_hold()),
        "orphan" => rt.block_on(orphan()),
        "bench" => bench::run(),
        _ => bail!("usage: s0 host|orphan|bench"),
    }
}

fn h<F, Fut>(f: F) -> Handler
where
    F: Fn(Peer, Incoming) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    Arc::new(move |p, i| Box::pin(f(p, i)))
}

// ---- module side ------------------------------------------------------------------

async fn module_main() -> Result<()> {
    // The control socket is stdin. Take our own CLOEXEC copy, then point fd 0 at
    // /dev/null so anything this module runs (psql, mariadb …) never inherits it.
    let ctl = std::io::stdin().as_fd().try_clone_to_owned()?;
    if std::env::var_os("S0_KEEP_STDIN").is_none() {
        let null = rustix::fs::open("/dev/null", rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC, rustix::fs::Mode::empty())?;
        rustix::stdio::dup2_stdin(&null)?;
    }
    let std_sock = std::os::unix::net::UnixStream::from(ctl);
    std_sock.set_nonblocking(true)?;
    let sock = UnixStream::from_std(std_sock)?;
    let peer = Peer::start(sock, h(serve));
    let closed = peer.closed.clone();
    let _: Text = peer.call(m::HELLO, &sample_hello()).await?;
    closed.notified().await;
    // Host gone: exit at once (no orphan).
    std::process::exit(0);
}

async fn serve(peer: Peer, inc: Incoming) {
    let r = serve_one(peer, inc).await;
    if let Err(e) = r {
        eprintln!("module: {e:#}");
    }
}

async fn serve_one(peer: Peer, inc: Incoming) -> Result<()> {
    let Incoming { method, body, fd, reply, cancelled } = inc;
    match method {
        m::ECHO => {
            let t: Text = minicbor::decode(&body)?;
            reply.ok(&t)?;
        }
        m::PLAN => {
            let req: PlanReq = minicbor::decode(&body)?;
            let mut plan = sample_spawn_plan();
            plan.specs[0].name = req.inst.name;
            reply.ok(&plan)?;
        }
        m::RESOLVE => {
            // Calls back into the host twice while the host's call is in flight.
            let mut digests = String::new();
            for full in ["17.6.0", "pgext-vector-pg17"] {
                let r: EnsureResp = peer
                    .call(m::FETCH_ENSURE, &EnsureReq { engine: "postgres".into(), full: full.into(), triple: "aarch64-apple-darwin".into(), expected_sha: String::new() })
                    .await?;
                digests.push_str(&r.digest);
                digests.push(' ');
            }
            reply.ok(&EnsureResp { bin_dir: "/toolchain/bin".into(), digest: digests.trim().into() })?;
        }
        m::TAIL => {
            let req: TailReq = minicbor::decode(&body)?;
            let mut cancelled = cancelled;
            let mut i = 0u32;
            loop {
                if req.rows != 0 && i == req.rows {
                    reply.end()?;
                    return Ok(());
                }
                let mut cells = std::collections::BTreeMap::new();
                cells.insert("n".to_string(), i.to_string());
                cells.insert("panel".to_string(), req.panel.clone());
                reply.item(&Row { cells, tone: String::new() })?;
                i += 1;
                if req.rows == 0 {
                    tokio::select! {
                        _ = &mut cancelled => {
                            let _: Text = peer.call(m::TAIL_CANCELLED, &Text { s: i.to_string() }).await?;
                            return Ok(());
                        }
                        _ = tokio::time::sleep(Duration::from_millis(10)) => {}
                    }
                }
            }
        }
        m::WIRE => {
            let fd = fd.context("wire call without an fd")?;
            let _opts: WireAccept = minicbor::decode(&body)?;
            let std_c = std::os::unix::net::UnixStream::from(fd);
            std_c.set_nonblocking(true)?;
            let client = UnixStream::from_std(std_c)?;
            let mut client = BufReader::new(client);
            let mut startup = String::new();
            client.read_line(&mut startup).await?;
            let backend: Text = peer.call(m::WIRE_BOOT, &Text { s: String::new() }).await?;
            let mut be = UnixStream::connect(&backend.s).await?;
            be.write_all(startup.as_bytes()).await?;
            let mut client = client.into_inner();
            let r = tokio::io::copy_bidirectional(&mut client, &mut be).await;
            let _: Text = peer.call(m::WIRE_RELEASE, &Text { s: String::new() }).await?;
            r?;
            reply.ok(&Text { s: startup.trim().to_string() })?;
        }
        m::CRASH => std::process::exit(3),
        m::SLEEP => {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            reply.ok(&Text { s: String::new() })?;
        }
        m::STDIN_CHECK => {
            let out = tokio::process::Command::new("sh")
                .arg("-c")
                .arg("if [ -S /dev/stdin ]; then echo socket; else echo not-a-socket; fi")
                .stdout(Stdio::piped())
                .output()
                .await?;
            reply.ok(&Text { s: String::from_utf8_lossy(&out.stdout).trim().to_string() })?;
        }
        _ => reply.err(&format!("unknown method {method}"))?,
    }
    Ok(())
}

// ---- host side --------------------------------------------------------------------

struct Launched {
    peer: Peer,
    child: tokio::process::Child,
    hello: Hello,
    launch: Duration,
}

struct HostState {
    acquired: AtomicI64,
    backend: String,
    tail_cancel: std::sync::Mutex<Option<oneshot::Sender<String>>>,
}

async fn launch(env: &[(&str, &str)], st: Arc<HostState>) -> Result<Launched> {
    let (ours, theirs) = std::os::unix::net::UnixStream::pair()?;
    let exe = std::env::current_exe()?;
    let t0 = Instant::now();
    let mut cmd = tokio::process::Command::new(exe);
    cmd.arg("module").stdin(Stdio::from(OwnedFd::from(theirs))).kill_on_drop(true);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let child = cmd.spawn()?;
    ours.set_nonblocking(true)?;
    let (htx, hrx) = oneshot::channel::<Hello>();
    let htx = Arc::new(std::sync::Mutex::new(Some(htx)));
    let handler = h(move |_peer, inc| {
        let htx = htx.clone();
        let st = st.clone();
        async move {
            let r: Result<()> = async {
                match inc.method {
                    m::HELLO => {
                        let hello: Hello = minicbor::decode(&inc.body)?;
                        if let Some(tx) = htx.lock().map_err(|_| anyhow!("poisoned"))?.take() {
                            let _ = tx.send(hello);
                        }
                        inc.reply.ok(&Text { s: "welcome".into() })?;
                    }
                    m::FETCH_ENSURE => {
                        let q: EnsureReq = minicbor::decode(&inc.body)?;
                        inc.reply.ok(&EnsureResp { bin_dir: format!("/cache/{}/bin", q.full), digest: format!("sha256:{}", q.full.len()) })?;
                    }
                    m::WIRE_BOOT => {
                        st.acquired.fetch_add(1, Ordering::SeqCst);
                        inc.reply.ok(&Text { s: st.backend.clone() })?;
                    }
                    m::WIRE_RELEASE => {
                        st.acquired.fetch_sub(1, Ordering::SeqCst);
                        inc.reply.ok(&Text { s: String::new() })?;
                    }
                    m::TAIL_CANCELLED => {
                        let t: Text = minicbor::decode(&inc.body)?;
                        if let Some(tx) = st.tail_cancel.lock().map_err(|_| anyhow!("poisoned"))?.take() {
                            let _ = tx.send(t.s);
                        }
                        inc.reply.ok(&Text { s: String::new() })?;
                    }
                    other => inc.reply.err(&format!("host: unknown method {other}"))?,
                }
                Ok(())
            }
            .await;
            if let Err(e) = r {
                eprintln!("host handler: {e:#}");
            }
        }
    });
    let peer = Peer::start(UnixStream::from_std(ours)?, handler);
    let hello = tokio::time::timeout(Duration::from_secs(5), hrx).await.context("no hello in 5 s")??;
    Ok(Launched { peer, child, hello, launch: t0.elapsed() })
}

fn pct(mut v: Vec<Duration>) -> (f64, f64, f64) {
    v.sort();
    let at = |q: f64| v[((v.len() as f64 - 1.0) * q) as usize].as_secs_f64() * 1e6;
    (at(0.5), at(0.99), at(0.999))
}

fn rss_kb(pid: u32) -> u64 {
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(0)
}

async fn fake_backend(path: &str) -> Result<()> {
    let _ = std::fs::remove_file(path);
    let l = tokio::net::UnixListener::bind(path)?;
    tokio::spawn(async move {
        while let Ok((s, _)) = l.accept().await {
            tokio::spawn(async move {
                let mut s = BufReader::new(s);
                let mut line = String::new();
                if s.read_line(&mut line).await.is_err() || !line.starts_with("STARTUP ") {
                    return;
                }
                let mut s = s.into_inner();
                if s.write_all(b"READY\n").await.is_err() {
                    return;
                }
                let (mut r, mut w) = s.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    Ok(())
}

async fn one_wire(peer: &Peer, n: u32) -> Result<()> {
    let token = format!("STARTUP {n}");
    let (client, server) = std::os::unix::net::UnixStream::pair()?;
    client.set_nonblocking(true)?;
    let mut client = UnixStream::from_std(client)?;
    let call = peer.call_fd::<_, Text>(m::WIRE, &WireAccept { require_tls: false, local_unix: true }, OwnedFd::from(server));
    let talk = async {
        client.write_all(format!("{token}\n").as_bytes()).await?;
        let mut buf = [0u8; 6];
        client.read_exact(&mut buf).await?;
        if &buf != b"READY\n" {
            bail!("backend said {:?}", String::from_utf8_lossy(&buf));
        }
        client.write_all(b"ping").await?;
        let mut p = [0u8; 4];
        client.read_exact(&mut p).await?;
        if &p != b"ping" {
            bail!("echo mismatch");
        }
        client.shutdown().await?;
        drop(client);
        Ok::<_, anyhow::Error>(())
    };
    let (r, t) = tokio::join!(call, talk);
    t?;
    let r = r?;
    if r.s != token {
        bail!("fd crossed: call for {token} came back as {}", r.s);
    }
    Ok(())
}

async fn host_main() -> Result<()> {
    let tmp = std::env::temp_dir().join(format!("s0-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;
    let backend = tmp.join("be.sock").to_string_lossy().to_string();
    fake_backend(&backend).await?;
    let st = Arc::new(HostState { acquired: AtomicI64::new(0), backend, tail_cancel: std::sync::Mutex::new(None) });

    // 1. launch → hello, cold-ish then warm.
    let mut launches = Vec::new();
    for _ in 0..20 {
        let l = launch(&[], st.clone()).await?;
        launches.push(l.launch);
        drop(l);
    }
    let (p50, p99, _) = pct(launches);
    let l = launch(&[], st.clone()).await?;
    println!("launch_to_hello_us p50={p50:.0} p99={p99:.0}  module={} caps={}", l.hello.module, l.hello.caps.len());
    let peer = l.peer.clone();
    tokio::time::sleep(Duration::from_millis(300)).await;
    println!("rss_kb idle: host={} module={}", rss_kb(std::process::id()), rss_kb(l.child.id().unwrap_or(0)));

    // 2. fd 0 hygiene.
    let t: Text = peer.call(m::STDIN_CHECK, &Text { s: String::new() }).await?;
    let leak = launch(&[("S0_KEEP_STDIN", "1")], st.clone()).await?;
    let t2: Text = leak.peer.call(m::STDIN_CHECK, &Text { s: String::new() }).await?;
    drop(leak);
    println!("grandchild_stdin with_dup2_null={} without={}", t.s, t2.s);

    // 3. sequential latency.
    let req = sample_plan_req();
    for _ in 0..1000 {
        let _: SpawnPlan = peer.call(m::PLAN, &req).await?;
    }
    let mut lat = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let t0 = Instant::now();
        let p: SpawnPlan = peer.call(m::PLAN, &req).await?;
        lat.push(t0.elapsed());
        if p.specs[0].name != "db" {
            bail!("wrong plan");
        }
    }
    let (p50, p99, p999) = pct(lat);
    println!("plan_call_us p50={p50:.1} p99={p99:.1} p999={p999:.1}  (req {} B, resp {} B)", minicbor::to_vec(&req)?.len(), minicbor::to_vec(sample_spawn_plan())?.len());
    let mut lat = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let t0 = Instant::now();
        let _: Text = peer.call(m::ECHO, &Text { s: "x".into() }).await?;
        lat.push(t0.elapsed());
    }
    let (p50, p99, _) = pct(lat);
    println!("echo_call_us p50={p50:.1} p99={p99:.1}");

    // 4. concurrency: 5000 calls, 100 in flight.
    let t0 = Instant::now();
    let mut tasks = Vec::new();
    for _ in 0..100 {
        let p = peer.clone();
        let req = req.clone();
        tasks.push(tokio::spawn(async move {
            for _ in 0..50 {
                let _: SpawnPlan = p.call(m::PLAN, &req).await?;
            }
            Ok::<_, anyhow::Error>(())
        }));
    }
    for t in tasks {
        t.await??;
    }
    println!("concurrent_5000_plan_ms {:.1}", t0.elapsed().as_secs_f64() * 1e3);

    // 5. callbacks: resolve → two host fetch calls inside one module call.
    let mut lat = Vec::new();
    for _ in 0..2000 {
        let t0 = Instant::now();
        let r: EnsureResp = peer.call(m::RESOLVE, &Text { s: String::new() }).await?;
        lat.push(t0.elapsed());
        if r.digest != "sha256:6 sha256:17" {
            bail!("resolve digest {}", r.digest);
        }
    }
    let (p50, p99, _) = pct(lat);
    println!("resolve_with_2_callbacks_us p50={p50:.1} p99={p99:.1}");

    // 6. streams: finite, then cancel mid-stream.
    let mut s = peer.stream(m::TAIL, &TailReq { panel: "sessions".into(), selector: String::new(), rows: 12 })?;
    let mut n = 0;
    while let Some(r) = s.next::<Row>().await {
        r?;
        n += 1;
    }
    let (ctx, crx) = oneshot::channel();
    *st.tail_cancel.lock().map_err(|_| anyhow!("poisoned"))? = Some(ctx);
    let mut s = peer.stream(m::TAIL, &TailReq { panel: "live".into(), selector: "orders".into(), rows: 0 })?;
    for _ in 0..5 {
        s.next::<Row>().await.context("stream ended early")??;
    }
    let t0 = Instant::now();
    s.cancel();
    let seen = tokio::time::timeout(Duration::from_secs(2), crx).await.context("module never saw the cancel")??;
    println!("tail rows={n}/12; cancel seen by module after {} rows in {:.2} ms", seen, t0.elapsed().as_secs_f64() * 1e3);

    // 7. wire handoff (SCM_RIGHTS on the control socket).
    for i in 0..50 {
        one_wire(&peer, i).await?;
    }
    let mut lat = Vec::new();
    for i in 0..1000 {
        let t0 = Instant::now();
        one_wire(&peer, i).await?;
        lat.push(t0.elapsed());
    }
    let (p50, p99, _) = pct(lat);
    let t0 = Instant::now();
    let mut tasks = Vec::new();
    for i in 0..200 {
        let p = peer.clone();
        tasks.push(tokio::spawn(async move { one_wire(&p, 10_000 + i).await }));
    }
    for t in tasks {
        t.await??;
    }
    println!(
        "wire_handoff_us p50={p50:.0} p99={p99:.0}; 200 concurrent in {:.1} ms; acquired-released balance={}",
        t0.elapsed().as_secs_f64() * 1e3,
        st.acquired.load(Ordering::SeqCst)
    );

    // 8. memory.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mpid = l.child.id().unwrap_or(0);
    println!("rss_kb host={} module={}", rss_kb(std::process::id()), rss_kb(mpid));

    // 9. module crash: in-flight call fails, and how fast.
    let sleeper = { let p = peer.clone(); tokio::spawn(async move { p.call::<_, Text>(m::SLEEP, &Text { s: String::new() }).await }) };
    tokio::time::sleep(Duration::from_millis(20)).await;
    let t0 = Instant::now();
    let r = peer.call::<_, Text>(m::CRASH, &Text { s: String::new() }).await;
    let crash_ms = t0.elapsed().as_secs_f64() * 1e3;
    let inflight = sleeper.await?;
    println!(
        "crash: call err={:?} after {crash_ms:.2} ms; in-flight call err={:?}",
        r.err().map(|e| e.to_string()),
        inflight.err().map(|e| e.to_string())
    );
    let mut child = l.child;
    let status = child.wait().await?;
    println!("crash: module exit status {status}");
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

// ---- host death ---------------------------------------------------------------------

async fn host_hold() -> Result<()> {
    let st = Arc::new(HostState { acquired: AtomicI64::new(0), backend: String::new(), tail_cancel: std::sync::Mutex::new(None) });
    let l = launch(&[], st).await?;
    // Keep the module busy: a live stream and a call that never returns.
    let mut s = l.peer.stream(m::TAIL, &TailReq { panel: "live".into(), selector: String::new(), rows: 0 })?;
    s.next::<Row>().await.context("no row")??;
    let p = l.peer.clone();
    tokio::spawn(async move { p.call::<_, Text>(m::SLEEP, &Text { s: String::new() }).await });
    println!("{}", l.child.id().unwrap_or(0));
    tokio::time::sleep(Duration::from_secs(3600)).await;
    drop(s);
    Ok(())
}

async fn orphan() -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut results = Vec::new();
    for _ in 0..10 {
        let mut hold = tokio::process::Command::new(&exe).arg("host-hold").stdout(Stdio::piped()).kill_on_drop(true).spawn()?;
        let out = hold.stdout.take().context("stdout")?;
        let mut line = String::new();
        BufReader::new(out).read_line(&mut line).await?;
        let mpid: i32 = line.trim().parse().context("module pid")?;
        let pid = rustix::process::Pid::from_raw(mpid).context("pid")?;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let t0 = Instant::now();
        hold.start_kill()?; // SIGKILL the host
        hold.wait().await?;
        let mut gone = None;
        while t0.elapsed() < Duration::from_secs(5) {
            if rustix::process::test_kill_process(pid).is_err() {
                gone = Some(t0.elapsed());
                break;
            }
            tokio::time::sleep(Duration::from_micros(200)).await;
        }
        results.push(gone);
    }
    let ms: Vec<String> = results.iter().map(|g| g.map(|d| format!("{:.1}", d.as_secs_f64() * 1e3)).unwrap_or_else(|| "ALIVE".into())).collect();
    println!("module gone after host SIGKILL (ms, 10 runs): {}", ms.join(" "));
    let (tx, _rx) = mpsc::channel::<()>(1);
    drop(tx);
    Ok(())
}
