//! W0 driver. `w0 host` runs the S0 scenarios through the platform layer; `w0 orphan`
//! kills the host and checks the module and an engine die; `w0 break` checks graceful
//! stop; `w0 pg` boots real postgres; `w0 which` checks executable lookup.

use anyhow::{Context, Result, anyhow, bail};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use w0_windows::m;
use w0_windows::msg::*;
use w0_windows::os;
use w0_windows::wire::{Handler, Incoming, Peer};

fn main() -> Result<()> {
    os::init()?;
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("");
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    match cmd {
        "module" => rt.block_on(module_main()),
        "host" => rt.block_on(host_main()),
        "host-hold" => rt.block_on(host_hold()),
        "engine" => {
            std::thread::sleep(Duration::from_secs(3600));
            Ok(())
        }
        "breakwait" => rt.block_on(breakwait()),
        "orphan" => rt.block_on(orphan()),
        "break" => rt.block_on(break_test()),
        "pg" => rt.block_on(pg()),
        "which" => which_test(),
        _ => bail!("usage: w0 host|orphan|break|pg|which"),
    }
}

fn h<F, Fut>(f: F) -> Handler
where
    F: Fn(Peer, Incoming) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    Arc::new(move |p, i| Box::pin(f(p, i)))
}

// ---- module -------------------------------------------------------------------------

async fn module_main() -> Result<()> {
    let conn = os::module_conn()?;
    let peer = Peer::start(conn, h(serve), 0);
    let closed = peer.closed.clone();
    let _: Text = peer.call(m::HELLO, &sample_hello()).await?;
    closed.notified().await;
    std::process::exit(0);
}

async fn serve(peer: Peer, inc: Incoming) {
    if let Err(e) = serve_one(peer, inc).await {
        eprintln!("module: {e:#}");
    }
}

async fn serve_one(peer: Peer, inc: Incoming) -> Result<()> {
    let Incoming { method, body, fd, reply, cancelled } = inc;
    match method {
        m::ECHO => reply.ok(&minicbor::decode::<Text>(&body)?)?,
        m::PLAN => {
            let req: PlanReq = minicbor::decode(&body)?;
            let mut plan = sample_spawn_plan();
            plan.specs[0].name = req.inst.name;
            reply.ok(&plan)?;
        }
        m::RESOLVE => {
            let mut digests = String::new();
            for full in ["17.6.0", "pgext-vector-pg17"] {
                let r: EnsureResp = peer
                    .call(m::FETCH_ENSURE, &EnsureReq { engine: "postgres".into(), full: full.into(), triple: "t".into(), expected_sha: String::new() })
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
            let p = fd.context("wire call without a handle")?;
            let std_c = os::tcp_from_passable(p);
            std_c.set_nonblocking(true)?;
            let mut client = BufReader::new(TcpStream::from_std(std_c)?);
            let mut startup = String::new();
            client.read_line(&mut startup).await?;
            let backend: Text = peer.call(m::WIRE_BOOT, &Text { s: String::new() }).await?;
            let mut be = TcpStream::connect(&backend.s).await?;
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
        _ => reply.err(&format!("unknown method {method}"))?,
    }
    Ok(())
}

// ---- host ---------------------------------------------------------------------------

struct HostState {
    acquired: AtomicI64,
    backend: String,
    tail_cancel: std::sync::Mutex<Option<oneshot::Sender<String>>>,
}

struct Launched {
    peer: Peer,
    child: tokio::process::Child,
    job: os::Job,
    hello: Hello,
    launch: Duration,
}

fn host_handler(st: Arc<HostState>, htx: oneshot::Sender<Hello>) -> Handler {
    let htx = Arc::new(std::sync::Mutex::new(Some(htx)));
    h(move |_peer, inc| {
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
    })
}

async fn launch(st: Arc<HostState>) -> Result<Launched> {
    let t0 = Instant::now();
    let mut cmd = tokio::process::Command::new(std::env::current_exe()?);
    cmd.arg("module").kill_on_drop(true);
    let (child, conn, job) = os::spawn_module(cmd).await?;
    let (htx, hrx) = oneshot::channel();
    let peer = Peer::start(conn, host_handler(st, htx), child.id().unwrap_or(0));
    let hello = tokio::time::timeout(Duration::from_secs(10), hrx).await.context("no hello in 10 s")??;
    Ok(Launched { peer, child, job, hello, launch: t0.elapsed() })
}

fn pct(mut v: Vec<Duration>) -> (f64, f64) {
    v.sort();
    let at = |q: f64| v[((v.len() as f64 - 1.0) * q) as usize].as_secs_f64() * 1e6;
    (at(0.5), at(0.99))
}

async fn fake_backend() -> Result<String> {
    let l = TcpListener::bind("127.0.0.1:0").await?;
    let addr = l.local_addr()?.to_string();
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
    Ok(addr)
}

/// The proxy's view: a client connected to doze's listener; hand the accepted socket to the module.
async fn one_wire(peer: &Peer, front: &std::net::TcpListener, n: u32) -> Result<()> {
    let token = format!("STARTUP {n}");
    let addr = front.local_addr()?;
    let mut client = TcpStream::connect(addr).await?;
    let (accepted, _) = front.accept()?;
    let call = peer.call_fd::<_, Text>(m::WIRE, &WireAccept { require_tls: false, local_unix: false }, os::passable_from_tcp(accepted));
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
        Ok::<_, anyhow::Error>(())
    };
    let (r, t) = tokio::join!(call, talk);
    t?;
    let r = r?;
    if r.s != token {
        bail!("handle crossed: call for {token} came back as {}", r.s);
    }
    Ok(())
}

async fn host_main() -> Result<()> {
    let backend = fake_backend().await?;
    let st = Arc::new(HostState { acquired: AtomicI64::new(0), backend, tail_cancel: std::sync::Mutex::new(None) });

    let mut launches = Vec::new();
    for _ in 0..20 {
        let l = launch(st.clone()).await?;
        launches.push(l.launch);
    }
    let (p50, p99) = pct(launches);
    let l = launch(st.clone()).await?;
    println!("launch_to_hello_us p50={p50:.0} p99={p99:.0} module={} caps={}", l.hello.module, l.hello.caps.len());
    let peer = l.peer.clone();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mpid = l.child.id().unwrap_or(0);
    println!("rss_kb idle: host={} module={}", os::rss_kb(std::process::id()), os::rss_kb(mpid));

    let req = sample_plan_req();
    for _ in 0..1000 {
        let _: SpawnPlan = peer.call(m::PLAN, &req).await?;
    }
    let mut lat = Vec::new();
    for _ in 0..10_000 {
        let t0 = Instant::now();
        let p: SpawnPlan = peer.call(m::PLAN, &req).await?;
        lat.push(t0.elapsed());
        if p.specs[0].name != "db" {
            bail!("wrong plan");
        }
    }
    let (p50, p99) = pct(lat);
    println!("plan_call_us p50={p50:.1} p99={p99:.1}");
    let mut lat = Vec::new();
    for _ in 0..10_000 {
        let t0 = Instant::now();
        let _: Text = peer.call(m::ECHO, &Text { s: "x".into() }).await?;
        lat.push(t0.elapsed());
    }
    let (p50, p99) = pct(lat);
    println!("echo_call_us p50={p50:.1} p99={p99:.1}");

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

    let mut lat = Vec::new();
    for _ in 0..2000 {
        let t0 = Instant::now();
        let r: EnsureResp = peer.call(m::RESOLVE, &Text { s: String::new() }).await?;
        lat.push(t0.elapsed());
        if r.digest != "sha256:6 sha256:17" {
            bail!("resolve digest {}", r.digest);
        }
    }
    let (p50, p99) = pct(lat);
    println!("resolve_with_2_callbacks_us p50={p50:.1} p99={p99:.1}");

    let mut s = peer.stream(m::TAIL, &TailReq { panel: "sessions".into(), selector: String::new(), rows: 12 })?;
    let mut n = 0;
    while let Some(r) = s.next::<Row>().await {
        r?;
        n += 1;
    }
    let (ctx, crx) = oneshot::channel();
    *st.tail_cancel.lock().map_err(|_| anyhow!("poisoned"))? = Some(ctx);
    let mut s = peer.stream(m::TAIL, &TailReq { panel: "live".into(), selector: String::new(), rows: 0 })?;
    for _ in 0..5 {
        s.next::<Row>().await.context("stream ended early")??;
    }
    let t0 = Instant::now();
    s.cancel();
    let seen = tokio::time::timeout(Duration::from_secs(2), crx).await.context("module never saw the cancel")??;
    println!("tail rows={n}/12; cancel seen after {seen} rows in {:.2} ms", t0.elapsed().as_secs_f64() * 1e3);

    let front = std::net::TcpListener::bind("127.0.0.1:0")?;
    for i in 0..50 {
        one_wire(&peer, &front, i).await?;
    }
    let mut lat = Vec::new();
    for i in 0..1000 {
        let t0 = Instant::now();
        one_wire(&peer, &front, i).await?;
        lat.push(t0.elapsed());
    }
    let (p50, p99) = pct(lat);
    // Concurrent: each task has its own front listener so accepts pair with their connects.
    let t0 = Instant::now();
    let mut tasks = Vec::new();
    for i in 0..200 {
        let p = peer.clone();
        tasks.push(tokio::spawn(async move {
            let front = std::net::TcpListener::bind("127.0.0.1:0")?;
            one_wire(&p, &front, 10_000 + i).await
        }));
    }
    for t in tasks {
        t.await??;
    }
    println!(
        "wire_handoff_us p50={p50:.0} p99={p99:.0}; 200 concurrent in {:.1} ms, 0 crossed; acquire/release balance={}",
        t0.elapsed().as_secs_f64() * 1e3,
        st.acquired.load(Ordering::SeqCst)
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    println!("rss_kb after load: host={} module={}", os::rss_kb(std::process::id()), os::rss_kb(mpid));

    let sleeper = {
        let p = peer.clone();
        tokio::spawn(async move { p.call::<_, Text>(m::SLEEP, &Text { s: String::new() }).await })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    let t0 = Instant::now();
    let r = peer.call::<_, Text>(m::CRASH, &Text { s: String::new() }).await;
    let crash_ms = t0.elapsed().as_secs_f64() * 1e3;
    let inflight = sleeper.await?;
    println!("crash: call err={:?} after {crash_ms:.2} ms; in-flight err={:?}", r.err().map(|e| e.to_string()), inflight.err().map(|e| e.to_string()));
    let mut child = l.child;
    println!("crash: module exit {}", child.wait().await?);
    drop(l.job);
    Ok(())
}

// ---- host death ----------------------------------------------------------------------

async fn host_hold() -> Result<()> {
    let st = Arc::new(HostState { acquired: AtomicI64::new(0), backend: String::new(), tail_cancel: std::sync::Mutex::new(None) });
    let l = launch(st).await?;
    // An "engine" the host started for an instance: it joins the same job.
    let mut eng = tokio::process::Command::new(std::env::current_exe()?);
    eng.arg("engine").stdin(Stdio::null());
    let engine = eng.spawn()?;
    l.job.assign(&engine)?;
    let mut s = l.peer.stream(m::TAIL, &TailReq { panel: "live".into(), selector: String::new(), rows: 0 })?;
    s.next::<Row>().await.context("no row")??;
    let p = l.peer.clone();
    tokio::spawn(async move { p.call::<_, Text>(m::SLEEP, &Text { s: String::new() }).await });
    println!("{} {}", l.child.id().unwrap_or(0), engine.id().unwrap_or(0));
    tokio::time::sleep(Duration::from_secs(3600)).await;
    drop((s, engine, l));
    Ok(())
}

async fn orphan() -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut mods = Vec::new();
    let mut engs = Vec::new();
    for _ in 0..10 {
        let mut hold = tokio::process::Command::new(&exe).arg("host-hold").stdout(Stdio::piped()).stdin(Stdio::null()).kill_on_drop(true).spawn()?;
        let out = hold.stdout.take().context("stdout")?;
        let mut line = String::new();
        BufReader::new(out).read_line(&mut line).await?;
        let mut it = line.split_whitespace().map(|x| x.parse::<u32>());
        let mpid = it.next().context("module pid")??;
        let epid = it.next().context("engine pid")??;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let t0 = Instant::now();
        hold.start_kill()?; // TerminateProcess / SIGKILL: the host gets no chance to clean up
        hold.wait().await?;
        let (mut mg, mut eg) = (None, None);
        while t0.elapsed() < Duration::from_secs(3) && (mg.is_none() || eg.is_none()) {
            if mg.is_none() && !os::alive(mpid) {
                mg = Some(t0.elapsed());
            }
            if eg.is_none() && !os::alive(epid) {
                eg = Some(t0.elapsed());
            }
            tokio::time::sleep(Duration::from_micros(500)).await;
        }
        mods.push(mg);
        engs.push(eg);
        if eg.is_none() {
            // Unix has no job: clean up the engine ourselves (it is ours, by exact pid).
            let _ = os::send_break(epid);
        }
    }
    let fmt = |v: &[Option<Duration>]| v.iter().map(|g| g.map(|d| format!("{:.1}", d.as_secs_f64() * 1e3)).unwrap_or_else(|| "ALIVE".into())).collect::<Vec<_>>().join(" ");
    println!("after host kill, module gone (ms): {}", fmt(&mods));
    println!("after host kill, engine gone (ms): {}", fmt(&engs));
    Ok(())
}

// ---- graceful stop ------------------------------------------------------------------

async fn breakwait() -> Result<()> {
    #[cfg(windows)]
    {
        let mut b = tokio::signal::windows::ctrl_break()?;
        b.recv().await;
    }
    #[cfg(unix)]
    {
        let mut t = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        t.recv().await;
    }
    std::process::exit(0);
}

async fn break_test() -> Result<()> {
    let mut c = tokio::process::Command::new(std::env::current_exe()?);
    c.arg("breakwait").stdin(Stdio::null()).kill_on_drop(true);
    os::new_group(&mut c);
    let mut child = c.spawn()?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let pid = child.id().context("pid")?;
    let t0 = Instant::now();
    match os::send_break(pid) {
        Ok(()) => match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
            Ok(s) => println!("graceful stop: exited {} in {:.1} ms", s?, t0.elapsed().as_secs_f64() * 1e3),
            Err(_) => {
                println!("graceful stop: NOT delivered (still running after 3 s)");
                child.start_kill()?;
            }
        },
        Err(e) => {
            println!("graceful stop: could not send ({e:#})");
            child.start_kill()?;
        }
    }
    Ok(())
}

// ---- real postgres --------------------------------------------------------------------

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

async fn run(bin: &std::path::Path, args: &[&str]) -> Result<std::process::Output> {
    Ok(tokio::process::Command::new(bin).args(args).stdin(Stdio::null()).output().await?)
}

async fn pg() -> Result<()> {
    let Some(bindir) = std::env::var_os("PGBIN").map(std::path::PathBuf::from) else {
        println!("pg: PGBIN not set, skipped");
        return Ok(());
    };
    let tmp = std::env::temp_dir().join(format!("w0-pg-{}", std::process::id()));
    let data = tmp.join("data");
    std::fs::create_dir_all(&tmp)?;
    let data_s = data.to_string_lossy().to_string();
    let t0 = Instant::now();
    let o = run(&bindir.join(exe("initdb")), &["-D", &data_s, "-U", "postgres", "-A", "trust", "-E", "UTF8", "--no-sync"]).await?;
    if !o.status.success() {
        bail!("initdb failed: {}", String::from_utf8_lossy(&o.stderr));
    }
    println!("pg: initdb {:.0} ms", t0.elapsed().as_secs_f64() * 1e3);
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0")?;
        l.local_addr()?.port()
    };
    let port_s = port.to_string();
    let job = os::Job::new()?;

    // 1. The supervisor runs postgres itself (what core does with a SpawnPlan).
    let t0 = Instant::now();
    let mut c = tokio::process::Command::new(bindir.join(exe("postgres")));
    c.args(["-D", &data_s, "-p", &port_s, "-c", "listen_addresses=127.0.0.1"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    os::new_group(&mut c);
    let mut child = c.spawn()?;
    job.assign(&child)?;
    let mut ready = None;
    while t0.elapsed() < Duration::from_secs(30) {
        if let Ok(Some(st)) = child.try_wait() {
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                e.read_to_string(&mut err).await?;
            }
            println!("pg: postgres exited {st} at once: {}", err.lines().take(3).collect::<Vec<_>>().join(" | "));
            break;
        }
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            let o = run(&bindir.join(exe("pg_isready")), &["-h", "127.0.0.1", "-p", &port_s, "-U", "postgres"]).await?;
            if o.status.success() {
                ready = Some(t0.elapsed());
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let direct = ready.is_some();
    if let Some(r) = ready {
        println!("pg: direct spawn ready over tcp in {:.0} ms", r.as_secs_f64() * 1e3);
    }

    // 2. Fallback if postgres refused to run directly (e.g. an elevated admin token): pg_ctl.
    if !direct {
        let t0 = Instant::now();
        let o = run(&bindir.join(exe("pg_ctl")), &["start", "-w", "-D", &data_s, "-o", &format!("-p {port} -c listen_addresses=127.0.0.1"), "-l", &tmp.join("pg.log").to_string_lossy()]).await?;
        println!("pg: pg_ctl start -> {} in {:.0} ms {}", o.status, t0.elapsed().as_secs_f64() * 1e3, String::from_utf8_lossy(&o.stderr).trim());
    }

    let o = run(&bindir.join(exe("psql")), &["-h", "127.0.0.1", "-p", &port_s, "-U", "postgres", "-tAc", "select version()"]).await?;
    println!("pg: psql -> {}", String::from_utf8_lossy(&o.stdout).trim().chars().take(60).collect::<String>());

    // Graceful stop: the module-declared stop command (D15), then measure.
    let t0 = Instant::now();
    let o = run(&bindir.join(exe("pg_ctl")), &["stop", "-D", &data_s, "-m", "fast", "-w"]).await?;
    println!("pg: pg_ctl stop -m fast -> {} in {:.0} ms", o.status, t0.elapsed().as_secs_f64() * 1e3);
    if direct {
        match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
            Ok(s) => println!("pg: supervised postgres exited {}", s?),
            Err(_) => println!("pg: supervised postgres still running after stop"),
        }
    }
    drop(job);
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

// ---- executable lookup ----------------------------------------------------------------

fn which(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        let pe = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        let mut v: Vec<String> = pe.split(';').filter(|s| !s.is_empty()).map(|s| s.to_ascii_lowercase()).collect();
        if std::path::Path::new(name).extension().is_some() {
            v.insert(0, String::new());
        }
        v
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let p = dir.join(format!("{name}{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn which_test() -> Result<()> {
    let tmp = std::env::temp_dir().join(format!("w0-which-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;
    let script = if cfg!(windows) {
        let p = tmp.join("hello.cmd");
        std::fs::write(&p, "@echo off\r\necho [%~1] [%~2]\r\n")?;
        p
    } else {
        let p = tmp.join("hello");
        std::fs::write(&p, "#!/bin/sh\necho \"[$1] [$2]\"\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))?;
        }
        p
    };
    let mut paths = vec![tmp.clone()];
    if let Some(pg) = std::env::var_os("PGBIN") {
        paths.push(pg.into());
    }
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    // SAFETY-free: set_var is unsafe in edition 2024 only for threads; this runs before any.
    unsafe { std::env::set_var("PATH", std::env::join_paths(paths)?) };
    for name in ["hello", "psql", if cfg!(windows) { "cmd" } else { "sh" }] {
        println!("which {name} -> {:?}", which(name));
    }
    let hello = which("hello").context("hello not found")?;
    let o = std::process::Command::new(&hello).args(["a b", "c"]).output()?;
    println!("run {} [\"a b\", \"c\"] -> {} {:?}", hello.display(), o.status, String::from_utf8_lossy(&o.stdout).trim());
    let _ = script;
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}
