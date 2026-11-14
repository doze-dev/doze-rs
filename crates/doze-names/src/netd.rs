//! The network daemon (feature `netd`, so programs that only claim names do not carry the tunnel).
//!
//! On macOS it is the one process that makes every address in the block reachable (`pool.rs` says why there is one,
//! `nat.rs` what it does to a packet). launchd starts it as root, because opening a tunnel and adding a route need
//! root; it does those two things, gives up root for good, and from then on only reads the registry and copies packets
//! as the user who ran setup.
//!
//! On every system it also holds the zone's resolver whenever no doze program does. A routed domain
//! (`/etc/resolver/dev.acme.com`) points here for as long as the route exists; with doze fully down the route used to
//! point at a port nobody listened on, so the real dev.acme.com was unreachable until a stack came up. The daemon runs
//! from boot, so it joins the same first-come loop as every program, takes the socket when none holds it, and a
//! program that starts later stands by behind it. On Linux that is all it does: a name's address is local there, and
//! the daemon runs as the user from the start.

use crate::nat::{packet_len, translate};
use crate::pool::Net;
use crate::resolver::{ResolverConfig, serve};
use crate::table::TableCache;
use crate::{Error, Registry};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// How the daemon signs the registry while it serves the resolver: what a peer logs when it stands by.
pub const OWNER: &str = "doze-netd";

/// Goes into the daemon's job (launchd plist, systemd unit), so a build whose daemon differs from the installed one
/// installs again. Go's were 2 (Linux) and 3 (macOS: reads `names.ports.json`); 4 also forwards https to the front door.
pub const VERSION_LINUX: &str = "2";
pub const VERSION_DARWIN: &str = "4";

/// The file beside the registry naming the daemon that serves it.
pub const SERVING_FILE: &str = "netd.pid";

const MTU: u16 = 16384;
/// For what the registry cannot say: a service that died without writing.
const TABLE_REFRESH: Duration = Duration::from_secs(1);

fn fail(what: &str, e: impl std::fmt::Display) -> Error {
    Error::Io(std::io::Error::other(format!("netd: {what}: {e}")))
}

/// The macOS daemon: tunnel, route, drop to `uid`/`gid`, then translate until killed. `registry` is the user's
/// `names.json`.
pub fn run_translating(registry: &Path, uid: u32, gid: u32) -> Result<(), Error> {
    let net = Net::native();
    if let Some(owner) = route_owner(&run_route(&[
        "-n",
        "get",
        &net.at(crate::pool::DYNAMIC_BASE).to_string(),
    ])) {
        let cidr = net.cidr();
        return Err(fail(
            "route",
            format!("{cidr} is already routed ({owner}): another program uses the block"),
        ));
    }
    let gw = net.gateway();
    let dev = tun_rs::DeviceBuilder::new()
        .ipv4(gw, 32, Some(gw))
        .mtu(MTU)
        .build_sync()
        .map_err(|e| fail("open the interface", e))?;
    let ifname = dev.name().map_err(|e| fail("name the interface", e))?;
    let out = std::process::Command::new("/sbin/route")
        .args(["-n", "add", "-net", &net.cidr(), "-interface", &ifname])
        .output()?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(fail("route add", msg));
    }
    // Root's work is done. Everything from here reads a file the user owns and handles the user's own packets.
    doze_os::privilege::drop_root(uid, gid).map_err(|e| fail("drop root", e))?;
    eprintln!(
        "netd: {} on {ifname}, translating from {} as uid {uid}",
        net.cidr(),
        registry.display()
    );
    write_serving(registry);
    serve_zone(registry)?;
    let cache = Arc::new(TableCache::new(registry, net));
    let ticker = Arc::clone(&cache);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(TABLE_REFRESH);
            ticker.refresh(true);
        }
    });
    let mut buf = vec![0u8; 65535];
    loop {
        let n = match dev.recv(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(fail("read", e)),
        };
        let Some(p) = buf.get_mut(..n) else { continue };
        if first_packet(p) {
            cache.refresh(false);
        }
        if !translate(p, gw.octets(), cache.current().as_ref()) {
            continue;
        }
        let len = packet_len(p);
        if let Some(out) = p.get(..len)
            && let Err(e) = dev.send(out)
            && e.raw_os_error() != Some(NO_BUFFERS)
        {
            return Err(fail("write", e));
        }
    }
}

/// ENOBUFS on macOS: the interface's queue is full; the packet is lost and TCP sends it again.
const NO_BUFFERS: i32 = 55;

/// The Linux daemon: the zone's resolver alone, as the user (the systemd unit says who), until killed.
pub fn run_zone(registry: &Path) -> Result<(), Error> {
    if doze_os::privilege::is_privileged()? {
        return Err(fail(
            "start",
            "runs as the user whose zone it answers, never as root (the unit says who)",
        ));
    }
    eprintln!(
        "netd: answering the zone from {} on {}",
        registry.display(),
        crate::resolver_addr()
    );
    serve_zone(registry)?;
    loop {
        std::thread::park();
    }
}

/// Starts the resolver on a thread of its own, from the registry at `registry`, standing by while a doze program holds
/// it. The address is the zone's, read from the environment like every peer's: a daemon started by hand for a private
/// zone has to be given that zone's `DOZE_ZONE_RESOLVER`, or it serves the real one from the wrong file.
fn serve_zone(registry: &Path) -> Result<(), Error> {
    let cfg = zone_config(registry);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    std::thread::spawn(move || {
        rt.block_on(async move {
            let _resolver = serve(cfg);
            std::future::pending::<()>().await;
        });
    });
    Ok(())
}

/// The daemon's resolver: the zone from the registry at `registry`, signed as [`OWNER`], logging to stderr.
pub fn zone_config(registry: &Path) -> ResolverConfig {
    let home = registry.parent().unwrap_or(Path::new("."));
    let mut cfg = ResolverConfig::from_registry(&Registry::open(home, OWNER));
    cfg.log = Arc::new(|line| eprintln!("netd: {line}"));
    cfg
}

/// Whether `p` opens a TCP connection (a SYN without ACK): the moment the table is checked against the files.
pub fn first_packet(p: &[u8]) -> bool {
    let Some(first) = p.first() else { return false };
    let ihl = usize::from(first & 0x0f) * 4;
    p.get(9) == Some(&6) && p.get(ihl + 13).is_some_and(|f| f & 0x12 == 0x02)
}

fn run_route(args: &[&str]) -> String {
    std::process::Command::new("/sbin/route")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// What `route -n get <ip>` says the address is routed to, when that is more specific than the default route; `None`
/// when the block is free to take.
pub fn route_owner(output: &str) -> Option<String> {
    let field = |key: &str| {
        output.lines().find_map(|line| {
            let (k, v) = line.trim().split_once(':')?;
            (k.trim() == key).then(|| v.trim().to_string())
        })
    };
    let dest = field("destination").filter(|d| d != "default")?;
    Some(format!(
        "{dest} on {}",
        field("interface").unwrap_or_default()
    ))
}

/// Records which home this daemon serves, where that home's programs look (a daemon started by hand is found the same
/// way as launchd's). Best effort: the daemon works without it.
fn write_serving(registry: &Path) {
    let Some(dir) = registry.parent() else { return };
    let pid = format!("{}\n", std::process::id());
    if let Err(e) = std::fs::write(dir.join(SERVING_FILE), pid) {
        eprintln!(
            "netd: cannot record that it serves {}: {e}",
            registry.display()
        );
    }
}

#[cfg(test)]
#[path = "netd_tests.rs"]
mod tests;
