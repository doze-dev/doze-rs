//! The module channel against a real child process: bytes both ways, handles that arrive with the frame that
//! announced them, and the module's exit when its host lets go.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use common::{frame, helper, take_frame};
use doze_os::channel::{self, Passable, Reader};
use std::io::Read as _;
use std::time::Duration;

async fn next_frame(r: &mut Reader, buf: &mut Vec<u8>) -> Vec<u8> {
    let mut chunk = vec![0u8; 64 << 10];
    loop {
        if let Some(p) = take_frame(buf) {
            return p;
        }
        let n = r.read(&mut chunk).await.unwrap();
        assert!(n > 0, "module closed the channel early");
        buf.extend_from_slice(&chunk[..n]);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn bytes_both_ways_then_the_module_exits_when_the_host_lets_go() {
    let (mut child, ch) = channel::spawn_module(helper("module")).await.unwrap();
    let (mut r, mut w) = ch.split();
    let mut buf = Vec::new();
    for msg in [b"one".as_slice(), b"", &[7u8; 100_000]] {
        w.write_all(&frame(0, msg)).await.unwrap();
        assert_eq!(next_frame(&mut r, &mut buf).await, msg);
    }
    drop((r, w));
    let st = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("module still running")
        .unwrap();
    assert!(st.success(), "module exit: {st}");
}

/// Fifty connections handed over back to back, before any answer is read, each with its own token: a handle paired
/// with the wrong frame would send a token down the wrong connection.
#[tokio::test(flavor = "current_thread")]
async fn handles_arrive_with_the_frames_that_carry_them() {
    let (_child, ch) = channel::spawn_module(helper("module")).await.unwrap();
    let (mut r, mut w) = ch.split();
    let front = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut clients = Vec::new();
    for i in 0..50 {
        let client = std::net::TcpStream::connect(front.local_addr().unwrap()).unwrap();
        let (accepted, _) = front.accept().unwrap();
        let token = format!("token-{i}");
        let exported = w.export(Passable::from(accepted)).unwrap();
        let mut payload = token.clone().into_bytes();
        payload.extend_from_slice(exported.trailer());
        w.write_exported(&frame(1, &payload), exported)
            .await
            .unwrap();
        clients.push((client, token));
    }
    let mut buf = Vec::new();
    for (mut client, token) in clients {
        assert_eq!(
            next_frame(&mut r, &mut buf).await,
            format!("ok:{token}").into_bytes()
        );
        let mut got = String::new();
        client.read_to_string(&mut got).unwrap();
        assert_eq!(got, token, "a handle crossed frames");
    }
}
