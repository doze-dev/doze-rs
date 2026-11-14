#![no_main]
//! Any bytes a client sends after a good hello, to a daemon serving an empty stack on a real socket: every request is
//! answered or refused, nothing panics (a panic on any of the daemon's threads aborts the run), and the daemon still
//! accepts the next client.

use doze_daemon::{Daemon, StackInfo};
use doze_engine::Engine;
use doze_os::local::LocalStream;
use libfuzzer_sys::fuzz_target;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

struct Live {
    sock: PathBuf,
    _rt: tokio::runtime::Runtime,
    _serving: doze_daemon::Serving,
}

fn live() -> &'static Live {
    static LIVE: OnceLock<Live> = OnceLock::new();
    LIVE.get_or_init(|| {
        std::panic::set_hook(Box::new(|p| {
            eprintln!("{p}");
            std::process::abort();
        }));
        let dir = std::env::temp_dir().join(format!("dzd-fuzz-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp folder");
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("a runtime");
        let _in = rt.enter();
        let engine = Engine::builder("fuzz", &dir)
            .build()
            .expect("an empty stack");
        let sock = dir.join("d.sock");
        let d = Daemon::at(
            engine,
            Default::default(),
            StackInfo::default(),
            sock.clone(),
        );
        let serving = d.serve().expect("serving");
        Live {
            sock,
            _rt: rt,
            _serving: serving,
        }
    })
}

fuzz_target!(|data: &[u8]| {
    let l = live();
    let mut c = LocalStream::connect(&l.sock).expect("the daemon accepts");
    c.set_read_timeout(Some(Duration::from_millis(100)))
        .expect("a timeout");
    c.write_all(b"{\"hello\":{\"proto\":2,\"client\":\"fuzz\"}}\n")
        .expect("hello");
    let _ = c.write_all(data);
    let _ = c.write_all(b"\n");
    let mut buf = [0u8; 8192];
    // Read what it answers until it goes quiet or closes.
    for _ in 0..50 {
        match c.read(&mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }
});
