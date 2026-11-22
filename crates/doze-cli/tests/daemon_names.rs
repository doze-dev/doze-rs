//! Names on the real daemon (core plan D2), in a zone of its own (its home, DNS address and front-door addresses), so
//! it needs no machine setup and touches nothing of the machine's real zone: the stack's name is taken, an app it
//! fronts is `<name>.<stack>.doze` in the shared registry and answered by the zone's DNS server, routed through the front
//! door to doze's listener, given an address of its own where the machine can (Linux) and 127.0.0.1 where it cannot;
//! an app added at run time gets its name and a removed one gives it back; a stopped daemon leaves no name behind.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code

use doze_daemon::ctl::{self, RunDir};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ask(sock: &Path, line: &str) -> serde_json::Value {
    let w = doze_os::local::LocalStream::connect(sock).unwrap();
    w.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let mut r = BufReader::new(w.try_clone().unwrap());
    let mut w = w;
    w.write_all(b"{\"hello\":{\"proto\":2,\"client\":\"test\"}}\n")
        .unwrap();
    let mut hello = String::new();
    r.read_line(&mut hello).unwrap();
    w.write_all(format!("{line}\n").as_bytes()).unwrap();
    let mut answer = String::new();
    r.read_line(&mut answer).unwrap();
    serde_json::from_str(&answer).unwrap()
}

fn free_tcp() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn free_udp() -> String {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string()
}

/// Asks the zone's DNS server at `server` for `host`'s address (one A question, the answer's last four bytes).
fn lookup(server: &str, host: &str) -> Option<Ipv4Addr> {
    let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in host.split('.') {
        q.push(u8::try_from(label.len()).unwrap());
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]);
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    s.send_to(&q, server).unwrap();
    let mut buf = [0u8; 512];
    let n = s.recv(&mut buf).ok()?;
    let answers = u16::from_be_bytes([buf[6], buf[7]]);
    (answers > 0 && n >= 4).then(|| Ipv4Addr::new(buf[n - 4], buf[n - 3], buf[n - 2], buf[n - 1]))
}

fn wait(what: &str, mut ok: impl FnMut() -> bool) {
    let t = Instant::now();
    while !ok() {
        assert!(t.elapsed() < Duration::from_secs(15), "{what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_stack_with_names_is_named_resolved_routed_and_gives_every_name_back() {
    if !Path::new("/bin/sh").exists() || cfg!(windows) {
        return;
    }
    let root = Temp(std::env::temp_dir().join(format!("dzn-{}", std::process::id())));
    let (project, data, home) = (root.0.join("p"), root.0.join("d"), root.0.join("h"));
    std::fs::create_dir_all(&project).unwrap();
    let (port, late_port) = (free_tcp(), free_tcp());
    let hcl = format!(
        "stack = \"d2names\"\ndata_dir = \"{}\"\n\ndefaults {{\n  names = true\n}}\n\napp \"api\" {{\n  run  = \"exec sleep 600\"\n  port = {port}\n}}\n",
        data.display(),
    );
    std::fs::write(project.join("doze.hcl"), hcl).unwrap();
    let dns = free_udp();
    let (front, front_tls) = (free_tcp(), free_tcp());
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_doze"));
    let mut child = std::process::Command::new(&exe)
        .args(["__daemon", "--config"])
        .arg(project.join("doze.hcl"))
        .env("DOZE_HOME", &home)
        .env("DOZE_ZONE_RESOLVER", &dns)
        .env("DOZE_ZONE_INGRESS", format!("127.0.0.1:{front}"))
        .env("DOZE_ZONE_INGRESS_TLS", format!("127.0.0.1:{front_tls}"))
        .env_remove("DOZE_ZONE")
        .stderr(std::fs::File::create(root.0.join("daemon.log")).unwrap())
        .spawn()
        .unwrap();
    // Reaped as it exits, as `up` detaches it: an exited child not yet waited for still counts as alive.
    let reaped = std::thread::spawn(move || child.wait());
    let run = RunDir::of(&data);
    wait("the daemon serves", || ctl::running(&run));

    let reg = doze_names::Registry::open(&home, "test");
    let host = "api.d2names.doze";
    let ip = reg.resolve(host).expect("api's name is in the registry");
    assert!(doze_names::on_this_machine(ip.into()), "{ip}");
    assert_eq!(
        reg.snapshot()
            .get("_stack.d2names")
            .map(|e| e.owner.clone()),
        Some("doze".to_string()),
        "the stack's name is taken"
    );
    // The name's address and port, as a client dials it: where addresses are translated (a set-up Mac) the network
    // daemon turns that into doze's listener, and drops a dial to the listener's own socket.
    let target = reg.front_target(host).expect("api is routed");
    assert_eq!(
        target,
        format!("{ip}:{port}"),
        "the front door goes to doze's listener"
    );
    wait("the zone answers", || lookup(&dns, host) == Some(ip));

    status_says_where(&run, host, &format!("{ip}:{port}"));
    added_and_removed(&run, &reg, late_port);

    assert_eq!(ctl::stop(&run), Ok(true));
    let _ = reaped.join();
    let left: Vec<String> = reg
        .snapshot()
        .into_keys()
        .filter(|k| k.contains("d2names"))
        .collect();
    assert!(left.is_empty(), "names left behind: {left:?}");
    let log = std::fs::read_to_string(root.0.join("daemon.log")).unwrap_or_default();
    assert!(log.contains("names: *.d2names.doze"), "{log}");
}

/// The status op names the app by its name, its URL through the front door, and where it really listens.
fn status_says_where(run: &RunDir, host: &str, endpoint: &str) {
    let status = ask(&run.socket(), r#"{"id":1,"op":"status"}"#);
    let api = status["response"]["instances"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "api")
        .cloned()
        .unwrap();
    assert_eq!(api["domain"], host, "{api}");
    assert_eq!(api["url"], format!("http://{host}"), "{api}");
    assert_eq!(api["endpoint"], endpoint, "{api}");
}

/// Added at run time: named at once; removed: the name goes with it.
fn added_and_removed(run: &RunDir, reg: &doze_names::Registry, late_port: u16) {
    let block = format!("app \"late\" {{\n  run  = \"exec sleep 600\"\n  port = {late_port}\n}}\n");
    let line = format!(
        "{{\"id\":2,\"op\":\"add\",\"block\":{}}}",
        serde_json::Value::String(block)
    );
    let added = ask(&run.socket(), &line);
    assert_eq!(added["response"]["ok"], true, "{added}");
    assert_eq!(
        added["response"]["instances"][0]["domain"], "late.d2names.doze",
        "{added}"
    );
    assert!(reg.resolve("late.d2names.doze").is_some());
    assert!(reg.front_target("late.d2names.doze").is_some());
    let removed = ask(&run.socket(), r#"{"id":3,"op":"remove","db":"late"}"#);
    assert!(removed.to_string().contains("\"ok\":true"), "{removed}");
    assert!(
        reg.lookup("late.d2names.doze").is_none(),
        "a removed app's name is given back"
    );
}
