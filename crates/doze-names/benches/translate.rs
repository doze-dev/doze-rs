//! What the macOS daemon's per-packet work costs, without root: `cargo bench -p doze-names --bench translate`.
//!
//! Each run copies a packet as the interface would hand it over (a client's segment to `db.shop.doze:5432`) and
//! translates it. The copy alone is timed too, so the translation's own share can be read off. The daemon also does a
//! `recv` and a `send` per packet, which no benchmark without the interface can time: on the owner's Mac those, not
//! this, are most of a packet's cost at 64 bytes.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::print_stdout)]

use doze_names::nat::{PortTable, Sum, translate_with};
use std::hint::black_box;
use std::time::Instant;

const GW: [u8; 4] = [198, 19, 0, 1];
const DB: [u8; 4] = [198, 19, 3, 7];

/// A valid IPv4 + TCP packet of `len` bytes from the interface's address to the service's public port.
fn packet(len: usize) -> Vec<u8> {
    let mut p = vec![0u8; len];
    p[0] = 0x45;
    p[2..4].copy_from_slice(&u16::try_from(len).unwrap().to_be_bytes());
    p[8] = 64;
    p[9] = 6;
    p[12..16].copy_from_slice(&GW);
    p[16..20].copy_from_slice(&DB);
    p[20..22].copy_from_slice(&50123u16.to_be_bytes());
    p[22..24].copy_from_slice(&5432u16.to_be_bytes());
    p[32] = 5 << 4;
    p[33] = 0x18; // PSH ACK
    for (i, b) in p.iter_mut().enumerate().skip(40) {
        *b = u8::try_from(i % 251).unwrap();
    }
    // Checksums as the kernel writes them: translate it once whole, with the source and destination swapped back.
    let mut t = PortTable::default();
    t.add(DB, 5432, 5432);
    let mut q = p.clone();
    q[12..16].copy_from_slice(&GW);
    q[16..20].copy_from_slice(&DB);
    assert!(translate_with(&mut q, GW, &t, Sum::Whole));
    // q is now DB -> GW with valid sums; swapping the addresses leaves both sums valid.
    q[12..16].copy_from_slice(&GW);
    q[16..20].copy_from_slice(&DB);
    q
}

fn per_second(len: usize, n: u32, mut f: impl FnMut(&mut [u8], &[u8])) -> f64 {
    let src = packet(len);
    let mut buf = src.clone();
    for _ in 0..n / 10 {
        f(&mut buf, &src);
    }
    let t = Instant::now();
    for _ in 0..n {
        f(&mut buf, &src);
        black_box(&buf);
    }
    f64::from(n) / t.elapsed().as_secs_f64()
}

fn main() {
    let mut table = PortTable::default();
    table.add(DB, 5432, 51000);
    println!(
        "{:>8}  {:>14}  {:>14}  {:>14}",
        "packet", "copy only", "whole sum", "adjusted sum"
    );
    for (len, n) in [
        (64usize, 20_000_000u32),
        (1500, 4_000_000),
        (16384, 400_000),
    ] {
        let copy = per_second(len, n, |b, s| b.copy_from_slice(s));
        let run = |sum: Sum| {
            per_second(len, n, |b, s| {
                b.copy_from_slice(s);
                assert!(translate_with(b, GW, &table, sum));
            })
        };
        let (whole, adjusted) = (run(Sum::Whole), run(Sum::Adjust));
        println!(
            "{:>6} B  {:>11.2} M/s  {:>11.2} M/s  {:>11.2} M/s",
            len,
            copy / 1e6,
            whole / 1e6,
            adjusted / 1e6
        );
    }
}
