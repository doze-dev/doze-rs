//! The macOS network daemon end to end (Go: e2e_darwin_test.go): real tunnel, real route, real packets.
//!
//! Skipped unless `DOZE_NAMES_E2E=foreground`, which `scripts/names-e2e-darwin.sh` sets after starting `doze-netd` as
//! root for a private `DOZE_HOME`. It never runs against ~/.doze.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_names::{Name, Net, Registry};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn e2e() -> Option<Registry> {
    if std::env::var("DOZE_NAMES_E2E").as_deref() != Ok("foreground") {
        eprintln!(
            "skipped: run scripts/names-e2e-darwin.sh (it starts the daemon for a private home)"
        );
        return None;
    }
    let net = Net::native();
    assert!(net.virtual_, "the translating daemon exists on macOS only");
    assert!(
        TcpListener::bind((net.gateway(), 0)).is_ok(),
        "the daemon's interface ({}) is not up",
        net.gateway()
    );
    let home =
        PathBuf::from(std::env::var("DOZE_HOME").expect("the script sets a private DOZE_HOME"));
    Some(Registry::open(&home, "e2e"))
}

fn serve_name(ln: TcpListener, name: String) {
    std::thread::spawn(move || {
        for c in ln.incoming() {
            let Ok(mut c) = c else { return };
            let _ = c.write_all(name.as_bytes());
        }
    });
}

fn ask(addr: SocketAddr) -> std::io::Result<String> {
    let mut c = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    c.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut s = String::new();
    c.read_to_string(&mut s)?;
    Ok(s)
}

/// TestNetworkThousandsOfServicesOnOnePort, at a count a laptop's open-file limit allows without raising it.
#[test]
fn hundreds_of_services_on_one_port_each_answer_as_themselves() {
    let Some(r) = e2e() else { return };
    let services = 500;
    let mut held = Vec::new();
    let start = Instant::now();
    for i in 0..services {
        let lease = r
            .claim(&Name::qualified("db", &format!("stack{i}")))
            .unwrap();
        let (socket, fwd) = lease.listen(5432).unwrap().into_parts();
        serve_name(socket, lease.name.host.clone());
        held.push((lease, fwd));
    }
    eprintln!(
        "{services} services on 5432 registered in {:?}",
        start.elapsed()
    );
    let start = Instant::now();
    for (lease, _) in &held {
        let got = ask(SocketAddr::from((lease.ip, 5432))).unwrap();
        assert_eq!(got, lease.name.host, "{}:5432", lease.ip);
    }
    eprintln!(
        "each connect+answer+close took {:?} on average",
        start.elapsed() / services
    );
}

/// A service that restarts on a new private port is reached on the very next connection.
#[test]
fn the_first_connection_after_a_restart_reaches_the_new_port() {
    let Some(r) = e2e() else { return };
    let lease = r.claim(&Name::qualified("api", "restart")).unwrap();
    let addr = SocketAddr::from((lease.ip, 8080));
    let first = lease.listen(8080).unwrap();
    let (socket, fwd) = first.into_parts();
    serve_name(socket, "first".into());
    assert_eq!(ask(addr).unwrap(), "first");
    drop(fwd);
    let (socket, _fwd) = lease.listen(8080).unwrap().into_parts();
    serve_name(socket, "second".into());
    assert_eq!(ask(addr).unwrap(), "second");
}

/// A port nobody registered is refused at once, not left to time out.
#[test]
fn an_unregistered_port_is_refused_at_once() {
    let Some(r) = e2e() else { return };
    let lease = r.claim(&Name::qualified("db", "refused")).unwrap();
    let start = Instant::now();
    let err =
        TcpStream::connect_timeout(&SocketAddr::from((lease.ip, 6379)), Duration::from_secs(5))
            .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::ConnectionRefused, "{err}");
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "took {:?}",
        start.elapsed()
    );
}

/// A name released mid-run stops being translated: its old private port is not reachable through the name.
#[test]
fn a_released_name_is_no_longer_reached() {
    let Some(r) = e2e() else { return };
    let lease = r.claim(&Name::qualified("db", "released")).unwrap();
    let (socket, fwd) = lease.listen(5432).unwrap().into_parts();
    serve_name(socket, "here".into());
    let addr = SocketAddr::from((lease.ip, 5432));
    assert_eq!(ask(addr).unwrap(), "here");
    drop(fwd);
    lease.release().unwrap();
    assert!(ask(addr).is_err(), "still reached after release");
}
