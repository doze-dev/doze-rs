//! A connection handed over with `wire.accept` arrives open. On macOS a socket whose only reference is the message
//! carrying it can be shut on the way by the kernel's collector of descriptors in flight (`doze_os::channel` has the
//! mechanism), so the host keeps its copy until the module shows it has the connection: its answer, or a callback
//! inside the call.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use common::{ALL, connected, handler, nothing};
use doze_module_proto::method::Method;
use doze_module_proto::msg::calls::{Unit, WireUse};
use doze_module_proto::msg::console::WireAccept;
use doze_module_proto::peer::{CallOpts, Handler, Incoming, Peer};
use doze_os::channel::{self, Passable};
use std::io::{ErrorKind, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, mpsc};

fn accept_args() -> WireAccept {
    WireAccept {
        instance: "e".into(),
        require_tls: false,
        local: true,
        tls: None,
    }
}

fn with(conn: UnixStream) -> CallOpts {
    CallOpts {
        handle: Some(Passable::from(conn)),
        ..CallOpts::default()
    }
}

/// Closes channels with a connection in flight, as fast as it can: on macOS each close runs the collector.
fn churn(stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        while !stop.load(Ordering::Relaxed) {
            rt.block_on(async {
                let (a, b) = channel::pair().await.unwrap();
                let (_r, mut w) = a.split();
                let (x, _y) = UnixStream::pair().unwrap();
                let ex = w.export(Passable::from(x)).unwrap();
                w.write_exported(b"x", ex).await.unwrap();
                drop((w, b));
            });
        }
    })
}

/// The module reads what the client sent before the hand-over and answers with it.
fn reader() -> Handler {
    handler(|mut c: Incoming| async move {
        let s: UnixStream = c.handle.take().unwrap().into();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut got = [0u8; 4];
        let n = (&s).read(&mut got).unwrap_or(0);
        let _ = c
            .responder
            .ok(&String::from_utf8_lossy(&got[..n]).into_owned());
    })
}

/// Before the fix about 1 in 60 of these arrived shut on macOS (the module's first read returned 0); none may.
#[tokio::test(flavor = "current_thread")]
async fn a_handed_over_connection_arrives_open_while_the_kernel_collects_descriptors() {
    const N: usize = 3000;
    let stop = Arc::new(AtomicBool::new(false));
    let churners: Vec<_> = (0..2).map(|_| churn(stop.clone())).collect();
    let (host, _m) = connected(ALL, nothing(), reader()).await;
    let mut shut = 0;
    for _ in 0..N {
        let (mut client, theirs) = UnixStream::pair().unwrap();
        client.write_all(b"ping").unwrap();
        let got: String = host
            .call(Method::WireAccept, &accept_args(), with(theirs))
            .await
            .unwrap();
        if got != "ping" {
            shut += 1;
        }
    }
    stop.store(true, Ordering::Relaxed);
    churners.into_iter().for_each(|c| c.join().unwrap());
    assert_eq!(
        shut, 0,
        "{shut} of {N} handed-over connections arrived shut"
    );
}

/// Whether the client's connection is closed at every end: its read returns 0 rather than timing out (the timeout
/// is set while the socket is open: macOS refuses socket options once the other end has closed).
fn closed(client: &UnixStream) -> bool {
    let mut r = client;
    match r.read(&mut [0u8; 1]) {
        Ok(0) => true,
        Ok(_) => panic!("unexpected bytes"),
        Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => false,
        Err(e) => panic!("{e}"),
    }
}

/// The module drops the connection at once, then waits for a go before each step: a callback inside the call
/// (`wire.acquire`, when told to), then its answer.
fn dropper(
    me: Arc<Mutex<Option<Peer>>>,
    go: Arc<Notify>,
    done: mpsc::UnboundedSender<()>,
) -> Handler {
    handler(move |mut c: Incoming| {
        let (me, go, done) = (
            me.lock().unwrap().clone().unwrap(),
            go.clone(),
            done.clone(),
        );
        async move {
            drop(c.handle.take());
            let callback: bool = c.args::<WireAccept>().unwrap().instance == "callback";
            if callback {
                go.notified().await;
                let opts = CallOpts {
                    for_call: Some(c.id()),
                    ..CallOpts::default()
                };
                let use_ = WireUse {
                    instance: "e".into(),
                };
                let _: Unit = me.call(Method::WireAcquire, &use_, opts).await.unwrap();
                done.send(()).unwrap();
            }
            go.notified().await;
            let _ = c.responder.ok(&Unit {});
        }
    })
}

/// Deterministic: the module has closed its copy, so the client sees the end exactly when the host lets go of its own.
#[tokio::test(flavor = "current_thread")]
async fn the_host_keeps_its_copy_until_the_module_shows_it_has_the_connection() {
    let slot: Arc<Mutex<Option<Peer>>> = Arc::default();
    let go = Arc::new(Notify::new());
    let (done_tx, mut done) = mpsc::unbounded_channel();
    let host_side = handler(|c: Incoming| async move {
        let _ = c.responder.ok(&Unit {});
    });
    let (host, m) = connected(ALL, host_side, dropper(slot.clone(), go.clone(), done_tx)).await;
    *slot.lock().unwrap() = Some(m);
    for instance in ["answer", "callback"] {
        let (client, theirs) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        let args = WireAccept {
            instance: instance.into(),
            ..accept_args()
        };
        let h = host.clone();
        let call = tokio::spawn(async move {
            h.call::<_, Unit>(Method::WireAccept, &args, with(theirs))
                .await
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !closed(&client),
            "{instance}: the host let go before the module showed it had the connection"
        );
        if instance == "callback" {
            go.notify_one();
            done.recv().await.unwrap();
            assert!(closed(&client), "callback: the host still holds its copy");
        }
        go.notify_one();
        call.await.unwrap().unwrap();
        assert!(
            closed(&client),
            "{instance}: the host still holds its copy after the answer"
        );
    }
}
